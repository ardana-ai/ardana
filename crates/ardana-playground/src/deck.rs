//! The playground's state: the editors, the picked model, the share link that loaded them and the last run.

use leptos::prelude::*;

use crate::api::{ApiClient, Exchange};
use crate::request::{self, Questions, Reply};
use crate::share::SharePayload;

/// The model a request names when nothing else is picked: the server's default model, as in Jev.
pub const DEFAULT_MODEL: &str = "jev-latest";

/// Every signal of the page; `Copy`, so each component takes it whole.
#[derive(Clone, Copy)]
pub struct Deck {
    pub state_text: RwSignal<String>,
    /// The `questions` editor text and the last map it parsed to: invalid text leaves the map as it was.
    pub questions_text: RwSignal<String>,
    pub questions: RwSignal<Questions>,
    pub questions_error: RwSignal<Option<String>>,
    pub model: RwSignal<String>,
    /// Why the URL's share link could not be loaded.
    pub share_error: RwSignal<Option<String>>,
    pub runner: Action<String, Exchange>,
    pub last: Memo<Option<LastRun>>,
}

/// The last `POST /v1/systemone`: what went over the wire, and the reply read from it (`Err` when no response came).
#[derive(Debug, Clone, PartialEq)]
pub struct LastRun {
    pub exchange: Exchange,
    pub reply: Result<Reply, String>,
    /// Counts runs, so each run's results are drawn afresh.
    pub number: usize,
}

impl Deck {
    pub fn new(client: ApiClient) -> Deck {
        let runner = Action::new_local(move |body: &String| {
            let client = client.clone();
            let body = body.clone();
            async move { client.systemone(body).await }
        });
        let version = runner.version();
        let last = Memo::new(move |_| {
            runner.value().get().map(|exchange| LastRun {
                reply: exchange
                    .response
                    .as_ref()
                    .map(|(status, body)| request::read_reply(*status, body))
                    .map_err(Clone::clone),
                exchange,
                number: version.get(),
            })
        });
        Deck {
            state_text: RwSignal::new(String::new()),
            questions_text: RwSignal::new(String::new()),
            questions: RwSignal::new(Questions::new()),
            questions_error: RwSignal::new(None),
            model: RwSignal::new(DEFAULT_MODEL.to_string()),
            share_error: RwSignal::new(None),
            runner,
            last,
        }
    }

    /// Replaces the questions text; the map follows when the text parses. Empty text is an empty map.
    pub fn set_questions_text(&self, text: String) {
        let parsed = if text.trim().is_empty() {
            Ok(Questions::new())
        } else {
            request::parse_questions(&text)
        };
        match parsed {
            Ok(map) => {
                self.questions.set(map);
                self.questions_error.set(None);
            }
            Err(err) => self.questions_error.set(Some(err)),
        }
        self.questions_text.set(text);
    }

    pub fn load_share(&self, share: Result<SharePayload, String>) {
        match share {
            Ok(payload) => {
                self.state_text.set(payload.document_text);
                self.set_questions_text(payload.prompts_text);
                if let Some(model) = payload.selected_models.into_iter().next() {
                    self.model.set(model);
                }
                self.share_error.set(None);
            }
            Err(err) => self.share_error.set(Some(err)),
        }
    }

    /// Whether RUN can send: the questions parse and no run is in flight.
    pub fn can_run(&self) -> bool {
        self.questions_error.with(Option::is_none) && !self.runner.pending().get()
    }

    /// Sends the editors as they are now.
    pub fn run(&self) {
        let blocked = self.questions_error.with_untracked(Option::is_some)
            || self.runner.pending().get_untracked();
        if blocked {
            return;
        }
        let body = self.questions.with_untracked(|questions| {
            request::body(
                &self.model.get_untracked(),
                &self.state_text.get_untracked(),
                questions,
            )
        });
        self.runner.dispatch(body);
    }
}

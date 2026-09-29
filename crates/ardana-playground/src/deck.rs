//! The playground's state: the editors, the picked model, the share link that loaded them and the last run.

use std::collections::HashMap;

use leptos::prelude::*;

use ardana_api::{ModelInfo, ModelsResponse, QuestionType, SystemOneRequest};
use serde_json::Value;

use crate::api::{ApiClient, Exchange};
use crate::builder::{self, KindMemory};
use crate::request::{self, Questions, Reply};
use crate::share::SharePayload;

/// The prefix of the model aliases share links from other playgrounds carry (`jev-latest`, `jev-1.12`); the API reads
/// them as its default model, and so does the picker.
const ALIAS_PREFIX: &str = "jev-";

/// Every signal of the page; `Copy`, so each component takes it whole.
#[derive(Clone, Copy)]
pub struct Deck {
    pub state_text: RwSignal<String>,
    /// The `questions` editor text and the last map it parsed to: invalid text leaves the map as it was.
    pub questions_text: RwSignal<String>,
    pub questions: RwSignal<Questions>,
    pub questions_error: RwSignal<Option<String>>,
    /// The question whose builder is open.
    pub editing: RwSignal<Option<String>>,
    /// The picked model; empty until `/v1/models` names the default, and a request then names none.
    pub model: RwSignal<String>,
    /// `/v1/models` as last listed: the pulled models, then the library models a first run pulls.
    pub models: RwSignal<Vec<ModelInfo>>,
    /// The model the last run named (none for the server's default).
    pub run_model: RwSignal<Option<String>>,
    /// Why the URL's share link could not be loaded.
    pub share_error: RwSignal<Option<String>>,
    /// Both editors as they were before a preset load or a removed question, until the next edit.
    pub previous: RwSignal<Option<(String, String)>>,
    /// Each question's criteria per type it has had, so switching its type back restores them.
    kinds: StoredValue<HashMap<String, KindMemory>>,
    /// A polite announcement when an input turns invalid or valid again.
    pub notice: RwSignal<String>,
    pub runner: Action<String, Exchange>,
    pub last: Memo<Option<LastRun>>,
    /// The request the last run sent, read back from its exact body.
    pub sent: Memo<Option<SystemOneRequest>>,
    /// Whether the state or the model differs from the last run's.
    pub inputs_changed: Memo<bool>,
    /// Whether anything RUN would send differs from the last run's request.
    pub stale: Memo<bool>,
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
        let sent = Memo::new(move |_| {
            last.with(|last| {
                let run = last.as_ref()?;
                serde_json::from_str::<SystemOneRequest>(&run.exchange.request).ok()
            })
        });
        let state_text = RwSignal::new(String::new());
        let model = RwSignal::new(String::new());
        let questions = RwSignal::new(Questions::new());
        let inputs_changed = Memo::new(move |_| {
            sent.with(|sent| {
                sent.as_ref().is_some_and(|sent| {
                    sent.model.as_deref() != Some(model.read().as_str())
                        || sent.state != request::state_value(&state_text.read())
                })
            })
        });
        let stale = Memo::new(move |_| {
            inputs_changed.get()
                || sent.with(|sent| {
                    sent.as_ref()
                        .is_some_and(|sent| questions.with(|q| *q != sent.questions))
                })
        });
        Deck {
            state_text,
            questions_text: RwSignal::new(String::new()),
            questions,
            questions_error: RwSignal::new(None),
            editing: RwSignal::new(None),
            model,
            models: RwSignal::new(Vec::new()),
            run_model: RwSignal::new(None),
            share_error: RwSignal::new(None),
            previous: RwSignal::new(None),
            kinds: StoredValue::new(HashMap::new()),
            notice: RwSignal::new(String::new()),
            runner,
            last,
            sent,
            inputs_changed,
            stale,
        }
    }

    /// Replaces the state text, as typing does.
    pub fn set_state_text(&self, text: String) {
        self.previous.set(None);
        self.state_text.set(text);
    }

    /// Replaces the questions text; the map follows when the text parses. Empty text is an empty map.
    pub fn set_questions_text(&self, text: String) {
        let parsed = if text.trim().is_empty() {
            Ok(Questions::new())
        } else {
            request::parse_questions(&text)
        };
        let was_invalid = self.questions_error.with_untracked(Option::is_some);
        match parsed {
            Ok(map) => {
                self.questions.set(map);
                self.questions_error.set(None);
                if was_invalid {
                    self.notice.set("Questions JSON is valid again".into());
                }
            }
            Err(err) => {
                if !was_invalid {
                    self.notice.set(err.clone());
                }
                self.questions_error.set(Some(err));
            }
        }
        self.questions_text.set(text);
        self.previous.set(None);
    }

    /// Applies a builder edit to the questions map and rewrites the questions text from it. A failed edit changes
    /// nothing.
    pub fn edit(
        &self,
        change: impl FnOnce(&mut Questions) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut questions = self.questions.get_untracked();
        change(&mut questions)?;
        self.set_questions_text(request::questions_text(&questions));
        Ok(())
    }

    /// Applies a builder edit to one question.
    pub fn edit_question(
        &self,
        id: &str,
        change: impl FnOnce(&mut Value) -> Result<(), String>,
    ) -> Result<(), String> {
        self.edit(|questions| change(questions.get_mut(id).ok_or("no such question")?))
    }

    /// Switches a question's type, restoring the criteria it had under that type before.
    pub fn switch_kind(&self, id: &str, kind: QuestionType) {
        let _ = self.edit_question(id, |spec| {
            self.kinds.update_value(|kinds| {
                builder::switch_kind(spec, kind, kinds.entry(id.to_string()).or_default())
            });
            Ok(())
        });
    }

    /// Renames a question; its remembered criteria and its open builder follow it.
    pub fn rename_question(&self, old: &str, new: &str) -> Result<(), String> {
        self.edit(|questions| builder::rename_question(questions, old, new))?;
        self.kinds.update_value(|kinds| {
            if let Some(memory) = kinds.remove(old) {
                kinds.insert(new.to_string(), memory);
            }
        });
        self.editing.set(Some(new.to_string()));
        Ok(())
    }

    /// Removes a question, keeping both editors as they were for Restore previous. Returns the id of the question
    /// that now holds its place, if any.
    pub fn remove_question(&self, id: &str) -> Option<String> {
        let mut next = None;
        self.keep_previous(|| {
            let _ = self.edit(|questions| {
                if let Some(index) = questions.get_index_of(id) {
                    questions.shift_remove_index(index);
                    next = questions.get_index(index).map(|(id, _)| id.clone());
                }
                Ok(())
            });
        });
        self.kinds.update_value(|kinds| {
            kinds.remove(id);
        });
        self.editing.set(None);
        next
    }

    /// Loads a preset into both editors, keeping them as they were for Restore previous.
    pub fn load_preset(&self, state_text: String, questions_text: String) {
        self.keep_previous(|| self.load(state_text, questions_text));
    }

    /// Puts back both editors as they were before the last preset load or removed question.
    pub fn restore_previous(&self) {
        if let Some((state, questions)) = self.previous.get_untracked() {
            self.load(state, questions);
        }
    }

    /// Runs `change`, then offers the editors as they were before it for Restore previous.
    fn keep_previous(&self, change: impl FnOnce()) {
        let before = (
            self.state_text.get_untracked(),
            self.questions_text.get_untracked(),
        );
        change();
        self.previous.set(Some(before));
    }

    /// Loads both editors, as a share link or a preset does.
    pub fn load(&self, state_text: String, questions_text: String) {
        self.set_state_text(state_text);
        self.set_questions_text(questions_text);
        self.kinds.set_value(HashMap::new());
        self.editing.set(None);
    }

    /// Loads a share link's editors and model; a link without a model, or with an alias, picks the default model.
    pub fn load_share(&self, share: Result<SharePayload, String>) {
        match share {
            Ok(payload) => {
                self.load(payload.document_text, payload.prompts_text);
                let model = payload
                    .selected_models
                    .into_iter()
                    .next()
                    .filter(|model| !model.starts_with(ALIAS_PREFIX))
                    .or_else(|| self.default_model_untracked());
                self.model.set(model.unwrap_or_default());
                self.share_error.set(None);
            }
            Err(err) => self.share_error.set(Some(err)),
        }
    }

    /// Takes a `/v1/models` list; while no model is picked, picks its default.
    pub fn set_models(&self, list: ModelsResponse) {
        self.models.set(list.models);
        if self.model.with_untracked(String::is_empty)
            && let Some(default) = self.default_model_untracked()
        {
            self.model.set(default);
        }
    }

    /// The model requests without one use: the one the list marks `x_default`, else its first pulled model.
    fn default_model_untracked(&self) -> Option<String> {
        self.models.with_untracked(|models| {
            models
                .iter()
                .find(|m| m.x_default)
                .or_else(|| models.iter().find(|m| m.pulled()))
                .map(|m| m.name.clone())
        })
    }

    /// The listed model `name` when it is not pulled yet: the first run naming it pulls it.
    pub fn unpulled(&self, name: &str) -> Option<ModelInfo> {
        self.models.with(|models| {
            models
                .iter()
                .find(|m| m.name == name && !m.pulled())
                .cloned()
        })
    }

    /// The request RUN would send now.
    pub fn request(&self) -> SystemOneRequest {
        self.questions.with(|questions| {
            request::request(&self.model.read(), &self.state_text.read(), questions)
        })
    }

    /// Whether RUN can send: the questions parse and no run is in flight.
    pub fn can_run(&self) -> bool {
        self.questions_error.with(Option::is_none) && !self.runner.pending().get()
    }

    /// Sends the editors as they are now; does nothing while RUN is held.
    pub fn run(&self) {
        if !untrack(|| self.can_run()) {
            return;
        }
        let request = untrack(|| self.request());
        self.run_model.set(request.model.clone());
        self.runner.dispatch(request::body(&request));
    }
}

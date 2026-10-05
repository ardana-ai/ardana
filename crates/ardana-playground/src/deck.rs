//! The playground's state: the editors, the picked model, the share link that loaded them and the last run.

use std::collections::HashMap;

use leptos::prelude::*;

use ardana_api::{ModelInfo, ModelsResponse, QuestionType, SystemOneRequest};
use serde_json::Value;

use crate::api::{ApiClient, Exchange, Place, Unanswered};
use crate::builder::{self, KindMemory};
use crate::engine::{self, Stage};
use crate::request::{self, Questions, Reply};
use crate::share::SharePayload;

/// The prefix of the model aliases share links from other playgrounds carry (`jev-latest`, `jev-1.12`); the API reads
/// them as its default model, and so does the picker.
const ALIAS_PREFIX: &str = "jev-";

/// What the picker's value of a model's "In browser" row adds to its name: a word after a space, which no model name
/// holds (`ardana pull` refuses whitespace in names), so the row never takes a server model's value.
const IN_BROWSER: &str = " in-browser";

/// The picker's value of the "In browser" row of `name`.
pub fn browser_value(name: &str) -> String {
    format!("{name}{IN_BROWSER}")
}

/// The model a picker value names, and whether it is the model's "In browser" row.
fn picked_row(value: &str) -> (&str, bool) {
    match value.strip_suffix(IN_BROWSER) {
        Some(name) => (name, true),
        None => (value, false),
    }
}

/// The picker row a page opens on, from `/v1/models`: the default model (`x_default`) when this server has pulled it;
/// else the browser default's "In browser" row ([`browser_default`], Q11); else the default model, or the first pulled
/// one.
fn opening_row(models: &[ModelInfo]) -> Option<String> {
    let default = models.iter().find(|m| m.x_default);
    if let Some(default) = default.filter(|m| m.pulled()) {
        return Some(default.name.clone());
    }
    if let Some(browser) = browser_default(models) {
        return Some(browser_value(&browser.name));
    }
    default
        .or_else(|| models.iter().find(|m| m.pulled()))
        .map(|m| m.name.clone())
}

/// The browser default (`x_browser_default`, Q11), when the list names one with a browser variant.
fn browser_default(models: &[ModelInfo]) -> Option<&ModelInfo> {
    models
        .iter()
        .find(|m| m.x_browser_default && m.x_browser.is_some())
}

/// The picker row a share link's model opens on: the "In browser" row of the listed model it names when this server
/// has not pulled that model and the tab can run it, since the tab is where it runs here; else the listed model's own
/// row; else the name as it is.
fn linked_row(models: &[ModelInfo], name: &str) -> String {
    match row(models, name) {
        Some(m) if !m.pulled() && m.x_browser.is_some() => browser_value(&m.name),
        Some(m) => m.name.clone(),
        None => name.to_string(),
    }
}

/// The `/v1/models` row a model name is (Q20): the one whose name it equals, ASCII case ignored. The page knows no
/// other name: it never splits one into a model and a tag, as the server and the ardana CLI read them.
fn row<'a>(models: &'a [ModelInfo], name: &str) -> Option<&'a ModelInfo> {
    models.iter().find(|m| m.name.eq_ignore_ascii_case(name))
}

/// Where Run answers a server row's pick `name`: on the server only a row it has pulled; with the ardana CLI every
/// other name, a row it has not pulled or one no row carries, as written, and in the `standalone` build, which no
/// server serves, every pick. No pick on a server (no list to open on) names no model: the server answers that with
/// its default model.
fn server_runs(models: &[ModelInfo], name: &str, standalone: bool) -> Runs {
    match row(models, name) {
        Some(model) if model.pulled() => Runs::Server,
        None if name.is_empty() && !standalone => Runs::Server,
        _ => Runs::Cli,
    }
}

/// How the ardana CLI reaches the model `name` where no server runs the pick: the `standalone` build's visitor installs
/// ardana; on a server, which ardana runs already, `ardana pull` adds any name but a row it has pulled (an "In browser"
/// row's model too), and a row it has pulled needs only `ardana run`.
fn handoff(models: &[ModelInfo], name: &str, standalone: bool) -> Handoff {
    if standalone {
        Handoff::Install
    } else if server_runs(models, name, standalone) == Runs::Server {
        Handoff::Run
    } else {
        Handoff::Pull
    }
}

/// Where Run answers the pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runs {
    /// `POST /v1/systemone`: a row this server has pulled, or no pick at all.
    Server,
    /// This tab, on the model's browser variant: an "In browser" row.
    Tab,
    /// Nowhere here: a row this server has not pulled, a name no row carries, or any server row of the standalone
    /// build. The page hands it over to the ardana CLI ([`Handoff`]), and Run sends nothing (Q2, Q20).
    Cli,
}

/// How the page hands a pick this server does not run ([`Runs::Tab`], [`Runs::Cli`]) over to the ardana CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handoff {
    /// The standalone build, which no server serves: ardana.ai's install line, then `ardana run` on the visitor's own
    /// machine.
    Install,
    /// A local server that has not pulled the model: `ardana pull` where the server runs, after which Run sends the
    /// model's server row here, then `ardana run`.
    Pull,
    /// A local server that has pulled the model (picked on its "In browser" row): `ardana run`.
    Run,
}

/// Why Run is held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// The questions JSON does not parse.
    Invalid,
    /// The picked model runs only with the ardana CLI ([`Runs::Cli`]) until `ardana pull` adds it to this local
    /// server ([`Handoff::Pull`]).
    Pull(String),
    /// The picked model runs only with the ardana CLI, on the visitor's own machine: no server serves the standalone
    /// build ([`Handoff::Install`]).
    Cli(String),
    /// Nothing is picked yet in the standalone build, whose list is the library document: it is still loading, or it
    /// did not arrive (`failed`, [`Deck::unlisted`]) and the page asks for it again when the tab comes back.
    Listing { failed: bool },
    /// There is no question to ask.
    Empty,
}

impl Held {
    /// The banner's words.
    pub fn text(&self) -> String {
        match self {
            Held::Invalid => "Questions JSON has an error".to_string(),
            Held::Pull(model) => {
                format!("This server has not pulled {model}: pull it with the ardana CLI")
            }
            Held::Cli(model) => format!("{model} runs with the ardana CLI on your machine"),
            Held::Listing { failed: true } => "The model library is unavailable".to_string(),
            Held::Listing { failed: false } => "Loading the model library".to_string(),
            Held::Empty => "Add a question to run".to_string(),
        }
    }
}

/// What Run sends, and where it is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub body: String,
    /// For a run in this tab: the model, the bytes of its browser files (`x_browser`), and whether the server holds
    /// them (`x_browser_pulled`).
    pub tab: Option<(String, u64, bool)>,
}

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
    /// The picked model; empty until `/v1/models` names the row to open on, and a request then names none.
    pub model: RwSignal<String>,
    /// Whether the pick is the model's "In browser" row: Run answers in this tab, on its browser variant.
    pub in_browser: RwSignal<bool>,
    /// Whether the pick waits for `/v1/models` to place it: the page's first pick, or a share link's.
    unplaced: StoredValue<bool>,
    /// Whether this is the standalone build, which no server serves ([`ApiClient::standalone`]).
    pub standalone: bool,
    /// `/v1/models` as last listed: the pulled models, then the library models this server has not pulled.
    pub models: RwSignal<Vec<ModelInfo>>,
    /// Whether the last list did not arrive (`ApiClient::models` failed): `models` holds the list before it, if any.
    pub unlisted: RwSignal<bool>,
    /// The model the last run named (none for the server's default).
    pub run_model: RwSignal<Option<String>>,
    /// Why the URL's share link could not be loaded.
    pub share_error: RwSignal<Option<String>>,
    /// Both editors as they were before a preset load, a removed question or a share link, until the next edit.
    pub previous: RwSignal<Option<Previous>>,
    /// Each question's criteria per type it has had, so switching its type back restores them.
    kinds: StoredValue<HashMap<String, KindMemory>>,
    /// A polite announcement when an input turns invalid or valid again, or a preset loads; each set is said, the same
    /// words again too.
    pub notice: RwSignal<String>,
    /// Where a run in this tab is, while one is in flight.
    pub stage: RwSignal<Option<Stage>>,
    /// Ends the run in this tab in flight ([`Deck::stop`]).
    stopper: StoredValue<Option<ActionAbortHandle>>,
    /// Whether the last run was stopped before it answered.
    pub stopped: RwSignal<bool>,
    /// Whether a run of the picked "In browser" row downloads nothing (`engine::kept`): the model it was asked for, and
    /// the answer.
    pub kept: RwSignal<Option<(String, bool)>>,
    pub runner: Action<Job, Exchange>,
    pub last: Memo<Option<LastRun>>,
    /// The request the last run sent, read back from its exact body.
    pub sent: Memo<Option<SystemOneRequest>>,
    /// Whether the state, the model or where it runs differs from the last run's.
    pub inputs_changed: Memo<bool>,
    /// Whether anything RUN would send differs from the last run's request.
    pub stale: Memo<bool>,
}

/// What Restore previous puts back: both editors, and the picker's row when what replaced them (a share link) picked
/// another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Previous {
    state: String,
    questions: String,
    pick: Option<String>,
}

/// The last `POST /v1/systemone`: what went over the wire, and the reply read from it (`Err` when no response came).
#[derive(Debug, Clone, PartialEq)]
pub struct LastRun {
    pub exchange: Exchange,
    pub reply: Result<Reply, Unanswered>,
    /// Counts runs, so each run's results are drawn afresh.
    pub number: usize,
}

impl LastRun {
    /// Whether the run was answered in this tab.
    pub fn in_tab(&self) -> bool {
        matches!(self.exchange.place, Place::Tab(_))
    }
}

impl Deck {
    pub fn new(client: ApiClient) -> Deck {
        let standalone = client.standalone();
        let stage = RwSignal::new(None);
        let kept = RwSignal::new(None);
        let runner = Action::new_local(move |job: &Job| {
            let client = client.clone();
            let job = job.clone();
            async move {
                match job.tab {
                    None => client.systemone(job.body).await,
                    Some((name, total, pulled)) => {
                        let exchange =
                            engine::run(&client, &name, total, pulled, job.body, stage).await;
                        // A run that started the model has every file: the next downloads nothing.
                        if matches!(exchange.place, Place::Tab(Some(_))) {
                            kept.set(Some((name, true)));
                        }
                        exchange
                    }
                }
            }
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
        let in_browser = RwSignal::new(false);
        let questions = RwSignal::new(Questions::new());
        let inputs_changed = Memo::new(move |_| {
            sent.with(|sent| {
                sent.as_ref().is_some_and(|sent| {
                    sent.model.as_deref() != Some(model.read().as_str())
                        || sent.state != request::state_value(&state_text.read())
                })
            }) || last.with(|last| {
                last.as_ref()
                    .is_some_and(|run| run.in_tab() != in_browser.get())
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
            in_browser,
            unplaced: StoredValue::new(true),
            standalone,
            models: RwSignal::new(Vec::new()),
            unlisted: RwSignal::new(false),
            run_model: RwSignal::new(None),
            share_error: RwSignal::new(None),
            previous: RwSignal::new(None),
            kinds: StoredValue::new(HashMap::new()),
            notice: RwSignal::new(String::new()),
            stage,
            stopper: StoredValue::new(None),
            stopped: RwSignal::new(false),
            kept,
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
        self.keep_previous(false, || {
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
        self.keep_previous(false, || self.load(state_text, questions_text));
    }

    /// Puts back both editors as they were before the last preset load, removed question or share link, and the
    /// picker's row a share link replaced.
    pub fn restore_previous(&self) {
        if let Some(previous) = self.previous.get_untracked() {
            self.load(previous.state, previous.questions);
            if let Some(pick) = previous.pick {
                self.pick(&pick);
            }
        }
    }

    /// Runs `change`, then offers the editors as they were before it for Restore previous, with the picker's row when
    /// `pick` (a change that picks another, once it is placed).
    fn keep_previous(&self, pick: bool, change: impl FnOnce()) {
        let before = Previous {
            state: self.state_text.get_untracked(),
            questions: self.questions_text.get_untracked(),
            pick: pick
                .then(|| untrack(|| self.picked()))
                .filter(|pick| !pick.is_empty()),
        };
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

    /// Loads a share link's editors and model. Once `/v1/models` is in, a link without a model, or with an alias, opens
    /// where the page opens, and a model on the row it runs on here ([`linked_row`]). Editors that hold other work
    /// than the link's stay one Restore previous away, with the row they had; the link they hold already leaves what
    /// Restore previous offers as it was.
    pub fn load_share(&self, share: Result<SharePayload, String>) {
        match share {
            Ok(payload) => {
                let differ = untrack(|| {
                    let (state, questions) = (self.state_text.read(), self.questions_text.read());
                    !(state.is_empty() && questions.is_empty())
                        && (*state != payload.document_text || *questions != payload.prompts_text)
                });
                let load = || {
                    self.load(payload.document_text, payload.prompts_text);
                    let model = payload
                        .selected_models
                        .into_iter()
                        .next()
                        .filter(|model| !model.starts_with(ALIAS_PREFIX));
                    self.model.set(model.unwrap_or_default());
                    self.in_browser.set(false);
                    self.unplaced.set_value(true);
                    self.place();
                };
                if differ {
                    self.keep_previous(true, load);
                } else {
                    let previous = self.previous.get_untracked();
                    load();
                    self.previous.set(previous);
                }
                self.share_error.set(None);
            }
            Err(err) => self.share_error.set(Some(err)),
        }
    }

    /// Takes a `/v1/models` list, and places the pick that waits for it.
    pub fn set_models(&self, list: ModelsResponse) {
        self.models.set(list.models);
        self.unlisted.set(false);
        self.place();
    }

    /// Places a pick that waits for `/v1/models`, once the list is in: no model opens on [`opening_row`], a share
    /// link's model on [`linked_row`].
    fn place(&self) {
        if !self.unplaced.get_value() || self.models.with_untracked(Vec::is_empty) {
            return;
        }
        let model = self.model.get_untracked();
        let row = self.models.with_untracked(|models| {
            if model.is_empty() {
                opening_row(models)
            } else {
                Some(linked_row(models, &model))
            }
        });
        if let Some(row) = row {
            self.pick(&row);
        }
        self.unplaced.set_value(false);
    }

    /// Picks the picker row whose value is `value`: a model on the server, or a model's "In browser" row. Another row
    /// stops a run in this tab.
    pub fn pick(&self, value: &str) {
        let (name, in_browser) = picked_row(value);
        let same = untrack(|| self.in_browser.get() == in_browser && *self.model.read() == name);
        if !same {
            self.stop();
        }
        self.model.set(name.to_string());
        self.in_browser.set(in_browser);
        self.unplaced.set_value(false);
        if in_browser {
            engine::prepare();
        } else {
            // The server's runs need nothing of a model run in this tab: its session (the weights on the GPU, or in
            // onnxruntime-web's memory) is released, and a later run in the tab loads it again from the kept files.
            engine::unload();
        }
    }

    /// The picker's value of the pick.
    pub fn picked(&self) -> String {
        let model = self.model.get();
        if self.in_browser.get() {
            browser_value(&model)
        } else {
            model
        }
    }

    /// The bytes a tab downloads to run the picked model's browser variant, while its "In browser" row is picked.
    pub fn browser_size(&self) -> Option<u64> {
        if !self.in_browser.get() {
            return None;
        }
        self.listed(&self.model.read())?.x_browser
    }

    /// The `/v1/models` row the name `name` is ([`row`]).
    pub fn listed(&self, name: &str) -> Option<ModelInfo> {
        self.models.with(|models| row(models, name).cloned())
    }

    /// How the ardana CLI reaches the pick, where no server runs it ([`Handoff`]).
    pub fn handoff(&self) -> Handoff {
        self.models.with(|models| {
            self.model
                .with(|model| handoff(models, model, self.standalone))
        })
    }

    /// The browser default's name, when the list names one that runs in a tab.
    pub fn browser_default(&self) -> Option<String> {
        self.models
            .with(|models| browser_default(models).map(|m| m.name.clone()))
    }

    /// What the ardana CLI downloads to pull the pick, or to run it first: the size of the row it is (`x_size`); none
    /// for a name no row carries.
    pub fn pull_size(&self) -> Option<u64> {
        self.models
            .with(|models| self.model.with(|model| row(models, model)?.x_size))
    }

    /// Where Run answers the pick: this tab for an "In browser" row, else [`server_runs`]: on the server only a row it
    /// has pulled, never pulled by a run.
    pub fn runs(&self) -> Runs {
        if self.in_browser.get() {
            return Runs::Tab;
        }
        self.models.with(|models| {
            self.model
                .with(|model| server_runs(models, model, self.standalone))
        })
    }

    /// The request RUN would send now.
    pub fn request(&self) -> SystemOneRequest {
        self.questions.with(|questions| {
            request::request(&self.model.read(), &self.state_text.read(), questions)
        })
    }

    /// Whether Run can send: nothing holds it ([`Deck::held`]) and no run is in flight.
    pub fn can_run(&self) -> bool {
        self.held().is_none() && !self.runner.pending().get()
    }

    /// Why Run is held, when it is: the questions do not parse, nothing is picked yet in the standalone build (its
    /// list, the library document, loading or unavailable), the pick runs only with the ardana CLI (here once it is
    /// pulled, or on the visitor's machine), or there is no question.
    pub fn held(&self) -> Option<Held> {
        if self.questions_error.with(Option::is_some) {
            Some(Held::Invalid)
        } else if self.standalone && self.model.with(String::is_empty) {
            Some(Held::Listing {
                failed: self.unlisted.get(),
            })
        } else if self.runs() == Runs::Cli {
            let model = self.model.get();
            Some(if self.standalone {
                Held::Cli(model)
            } else {
                Held::Pull(model)
            })
        } else if self.questions.with(|q| q.is_empty()) {
            Some(Held::Empty)
        } else {
            None
        }
    }

    /// Sends the editors as they are now, to the server or, with an "In browser" row picked, to this tab's engine;
    /// does nothing while RUN is held.
    pub fn run(&self) {
        if !untrack(|| self.can_run()) {
            return;
        }
        let request = untrack(|| self.request());
        self.run_model.set(request.model.clone());
        let tab = untrack(|| {
            self.in_browser.get().then(|| {
                let name = self.model.get();
                let size = self.browser_size().unwrap_or_default();
                let pulled = self.listed(&name).is_some_and(|m| m.x_browser_pulled);
                (name, size, pulled)
            })
        });
        let in_tab = tab.is_some();
        if in_tab {
            // Set before the run starts, so the page never shows a server's wait for a run in this tab.
            self.stage.set(Some(Stage::Asking));
        }
        self.stopped.set(false);
        let handle = self.runner.dispatch(Job {
            body: request::body(&request),
            tab,
        });
        self.stopper.set_value(in_tab.then_some(handle));
    }

    /// Runs a share link once, as `?autorun=1` asks: at once on the server; in this tab only when its files need no
    /// download, since a first run there downloads what nobody has agreed to yet (Run then waits for a tap).
    pub fn autorun(&self) {
        if untrack(|| self.runs()) != Runs::Tab {
            self.run();
            return;
        }
        let deck = *self;
        let name = self.model.get_untracked();
        leptos::task::spawn_local(async move {
            let kept = engine::kept(&name).await;
            // The pick may have moved on meanwhile: only the row the link opened on runs.
            if kept && deck.in_browser.get_untracked() && deck.model.get_untracked() == name {
                deck.run();
            }
        });
    }

    /// Stops the run in this tab in flight, if any: its downloads end where they are, Run is Run again, and nothing it
    /// would have answered lands.
    pub fn stop(&self) {
        if self.stage.get_untracked().is_none() {
            return;
        }
        if let Some(handle) = self.stopper.try_update_value(Option::take).flatten() {
            handle.abort();
        }
        engine::stop();
        self.stage.set(None);
        self.stopped.set(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_values_name_the_model_and_where_it_runs() {
        assert_eq!(
            picked_row(&browser_value("decider:0.8b")),
            ("decider:0.8b", true)
        );
        assert_eq!(picked_row("decider:0.8b"), ("decider:0.8b", false));
        assert_eq!(picked_row("decider:2b-q8_0"), ("decider:2b-q8_0", false));
        assert_eq!(
            picked_row(&browser_value("gemma-4:26b-a4b")),
            ("gemma-4:26b-a4b", true)
        );
        assert_ne!(browser_value("decider:2b"), "decider:2b");
    }

    /// A `/v1/models` entry: `name`, pulled or not, the default model or not, and with a browser variant (the browser
    /// default when `browser` is `Some(true)`) or not.
    fn info(name: &str, pulled: bool, default: bool, browser: Option<bool>) -> ModelInfo {
        ModelInfo {
            name: name.to_string(),
            description: String::new(),
            release_date: String::new(),
            x_pulled: Some(pulled),
            x_default: default,
            x_size: None,
            x_browser: browser.map(|_| 1),
            x_browser_pulled: false,
            x_browser_default: browser == Some(true),
        }
    }

    /// Q20: a name is the row whose name it equals, ASCII case ignored, and nothing else: no family, alias, quant or
    /// former name of a row is that row.
    #[test]
    fn a_name_is_a_listed_row_and_nothing_else() {
        let models = [
            info("decider:2b", true, true, Some(false)),
            info("decider:2b-q8_0", true, false, None),
            info("decider:0.8b", false, false, Some(true)),
            info("gemma-4:26b-a4b", false, false, None),
        ];
        let named = |name| row(&models, name).map(|m| m.name.as_str());
        for (name, listed) in [
            ("decider:2b", "decider:2b"),
            ("Decider:2B", "decider:2b"),
            ("DECIDER:2B-Q8_0", "decider:2b-q8_0"),
            ("decider:0.8b", "decider:0.8b"),
            ("Gemma-4:26B-A4B", "gemma-4:26b-a4b"),
        ] {
            assert_eq!(named(name), Some(listed), "{name}");
        }
        for name in [
            "decider",
            "decider:latest",
            "decider:2b-q4_k_m",
            "decider:0.8b-q8_0",
            "decider-2b",
            "decider:2",
            "gemma-4:26b",
            "nomodel",
            "",
        ] {
            assert_eq!(named(name), None, "{name}");
        }
    }

    /// Q11: the page opens on the default model when the server has pulled it, else on the browser default's row in
    /// the tab (an empty registry, and the standalone build's library, which lists as one).
    #[test]
    fn the_page_opens_where_a_model_runs() {
        let local = [
            info("decider:2b", true, true, Some(false)),
            info("decider:0.8b", false, false, Some(true)),
            info("decider:4b", false, false, None),
        ];
        assert_eq!(opening_row(&local).as_deref(), Some("decider:2b"));
        let empty = [
            info("decider:2b", false, true, Some(false)),
            info("decider:0.8b", false, false, Some(true)),
            info("decider:4b", false, false, None),
        ];
        assert_eq!(opening_row(&empty), Some(browser_value("decider:0.8b")));
        // Without a browser default: the default model, else the first pulled one.
        let plain = [
            info("decider:4b", false, true, None),
            info("smollm3:3b", true, false, None),
        ];
        assert_eq!(opening_row(&plain).as_deref(), Some("decider:4b"));
        assert_eq!(opening_row(&plain[1..]).as_deref(), Some("smollm3:3b"));
        assert_eq!(opening_row(&[]), None);

        // A share link's model opens in the tab only where this server lacks it and the tab can run it, in any case;
        // a row named in other letters opens on that row; any other name opens as it is written.
        for name in ["decider:0.8b", "DECIDER:0.8B"] {
            assert_eq!(
                linked_row(&empty, name),
                browser_value("decider:0.8b"),
                "{name}"
            );
        }
        assert_eq!(linked_row(&local, "decider:2b"), "decider:2b");
        assert_eq!(linked_row(&local, "Decider:2B"), "decider:2b");
        assert_eq!(linked_row(&empty, "decider:4b"), "decider:4b");
        assert_eq!(linked_row(&empty, "Decider:4B"), "decider:4b");
        for name in [
            "decider",
            "Decider:0.8b-Q8_0",
            "decider:4b-q8_0",
            "decider-2b",
            "speed_latest",
        ] {
            assert_eq!(linked_row(&empty, name), name);
        }
    }

    /// Run answers on the server only a row it has pulled (Q20): every other name, a row it has not pulled or one no
    /// row carries (a family, a tag of another quant, a former name, anything), goes to the ardana CLI as written,
    /// autorun included, so a run never makes the server pull; the standalone build runs nothing on a server.
    #[test]
    fn run_never_makes_the_server_pull() {
        // decider:2b pulled (the default), and its Q8_0 too; the rest of the library not.
        let local = [
            info("decider:2b", true, true, Some(false)),
            info("decider:2b-q8_0", true, false, None),
            info("decider:0.8b", false, false, Some(true)),
            info("decider:4b", false, false, None),
        ];
        let spellings = [
            ("decider:2b", Runs::Server),
            ("Decider:2B", Runs::Server),
            ("decider:2b-q8_0", Runs::Server),
            ("DECIDER:2B-Q8_0", Runs::Server),
            ("decider", Runs::Cli),
            ("decider:latest", Runs::Cli),
            ("decider:2b-q4_k_m", Runs::Cli),
            ("decider:2b-q4_0", Runs::Cli),
            ("decider:4b", Runs::Cli),
            ("Decider:4B", Runs::Cli),
            ("decider:4b-q8_0", Runs::Cli),
            ("decider:0.8b", Runs::Cli),
            ("DECIDER:0.8B", Runs::Cli),
            ("decider-2b", Runs::Cli),
            ("nomodel", Runs::Cli),
            ("hf.co/Mapika/decider-2b-GGUF:Q4_K_M", Runs::Cli),
        ];
        for (name, runs) in spellings {
            assert_eq!(server_runs(&local, name, false), runs, "local {name}");
        }
        // No pick names no model: the server's default answers it.
        assert_eq!(server_runs(&local, "", false), Runs::Server);
        let library = [
            info("decider:2b", false, true, Some(false)),
            info("decider:0.8b", false, false, Some(true)),
            info("decider:4b", false, false, None),
        ];
        for (name, _) in spellings {
            assert_eq!(
                server_runs(&library, name, true),
                Runs::Cli,
                "standalone {name}"
            );
        }
        assert_eq!(server_runs(&library, "", true), Runs::Cli);
    }

    /// A pick this server does not run goes to the ardana CLI: on a local server, which ardana runs already, after
    /// `ardana pull` for any name but a row it has pulled (an "In browser" row's model too), and without it for an "In
    /// browser" row of a model it has pulled; in the standalone build, after ardana.ai's install line.
    #[test]
    fn the_cli_gets_a_model_onto_a_local_server() {
        let local = [
            info("decider:2b", true, true, Some(false)),
            info("decider:0.8b", false, false, Some(true)),
            info("decider:4b", false, false, None),
        ];
        for name in [
            "decider:4b",
            "Decider:4B",
            "decider:4b-q8_0",
            "decider:2b-q8_0",
            "decider:0.8b",
            "decider",
            "nomodel",
        ] {
            assert_eq!(handoff(&local, name, false), Handoff::Pull, "{name}");
        }
        assert_eq!(handoff(&local, "decider:2b", false), Handoff::Run);
        assert_eq!(handoff(&local, "Decider:2B", false), Handoff::Run);
        let library = local.clone().map(|m| ModelInfo {
            x_pulled: Some(false),
            ..m
        });
        for name in ["decider:4b", "decider:2b", "decider:0.8b", "speed_latest"] {
            assert_eq!(handoff(&library, name, true), Handoff::Install, "{name}");
        }
        assert_eq!(
            browser_default(&library).map(|m| m.name.as_str()),
            Some("decider:0.8b")
        );
        assert_eq!(browser_default(&local[..1]), None);
    }

    #[test]
    fn the_banner_says_why_run_is_held() {
        assert_eq!(
            Held::Pull("decider:4b".into()).text(),
            "This server has not pulled decider:4b: pull it with the ardana CLI"
        );
        assert_eq!(
            Held::Cli("decider:4b".into()).text(),
            "decider:4b runs with the ardana CLI on your machine"
        );
        assert_eq!(Held::Empty.text(), "Add a question to run");
    }
}

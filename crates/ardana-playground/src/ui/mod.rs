//! The workspace: a sidebar of settings and pages on the left, and on the right the sticky top bar with Run over one
//! page of blocks: the state on the left, the questions with their answers on the right, then the raw exchange and
//! the snippets as toggle blocks.

mod builder;
mod controls;
mod exchange;
mod figure;
mod icons;
mod questions;
mod sidebar;
mod snippets;
mod state;
mod topbar;

use leptos::ev;
use leptos::prelude::*;

use wasm_bindgen::JsCast;

use crate::api::ApiClient;
use crate::deck::Deck;
use crate::request::Reply;
use crate::share;

use icons::Icon;

/// The `localStorage` key that remembers a collapsed sidebar.
const SIDEBAR_KEY: &str = "ardana.sidebar";
/// Below this width the sidebar is a drawer over the page and the columns stack.
const NARROW: &str = "(max-width: 960px)";

/// The shell's own state: the sidebar collapsed on a wide screen, or open as a drawer on a narrow one.
#[derive(Clone, Copy)]
pub struct Shell {
    pub collapsed: RwSignal<bool>,
    pub drawer: RwSignal<bool>,
}

impl Shell {
    fn new() -> Shell {
        let collapsed = storage()
            .and_then(|s| s.get_item(SIDEBAR_KEY).ok().flatten())
            .is_some_and(|v| v == "collapsed");
        Shell {
            collapsed: RwSignal::new(collapsed),
            drawer: RwSignal::new(false),
        }
    }

    /// The sidebar's own close control: shuts the drawer on a narrow screen, else collapses the sidebar.
    pub fn close(&self) {
        if matches(NARROW) {
            self.drawer.set(false);
            focus_later("sidebar-opener".to_string());
        } else {
            self.collapsed.set(true);
            focus_later("sidebar-opener".to_string());
        }
    }

    /// The top bar's opener: opens the drawer on a narrow screen, else expands the sidebar.
    pub fn open(&self) {
        if matches(NARROW) {
            self.drawer.set(true);
        } else {
            self.collapsed.set(false);
        }
        focus_later("sidebar-close".to_string());
    }

    /// Ctrl+\ or Cmd+\: the sidebar's keyboard toggle.
    fn toggle(&self) {
        if matches(NARROW) {
            if self.drawer.get_untracked() {
                self.close()
            } else {
                self.open()
            }
        } else if self.collapsed.get_untracked() {
            self.open()
        } else {
            self.close()
        }
    }

    /// After a pick in the drawer, the drawer goes away; says whether it was open, so the caller moves focus off
    /// the row that is about to hide.
    pub fn settle(&self) -> bool {
        let was_open = self.drawer.get_untracked();
        if was_open {
            self.drawer.set(false);
        }
        was_open
    }
}

#[component]
pub fn App() -> impl IntoView {
    let client = ApiClient::same_origin();
    let deck = Deck::new(client.clone());
    let shell = Shell::new();
    let models = LocalResource::new(move || {
        let client = client.clone();
        async move { client.models().await }
    });

    // The URL is the outside world: load its share link now and whenever the fragment changes.
    let load_hash = move || {
        let hash = window().location().hash().unwrap_or_default();
        if let Some(share) = share::from_hash(&hash) {
            deck.load_share(share);
        }
    };
    load_hash();
    let hash_listener = window_event_listener(ev::hashchange, move |_| load_hash());
    on_cleanup(move || hash_listener.remove());

    // Ctrl/Cmd+Enter presses Run from anywhere on the page; Ctrl/Cmd+\ toggles the sidebar; Escape shuts the drawer
    // and dismisses any tooltip until the pointer or focus moves again.
    let keys = window_event_listener(ev::keydown, move |event| {
        let modifier = event.ctrl_key() || event.meta_key();
        match event.key().as_str() {
            "Enter" if modifier => {
                event.prevent_default();
                deck.run();
            }
            "\\" if modifier => {
                event.prevent_default();
                shell.toggle();
            }
            "Escape" => {
                tips(false);
                if shell.drawer.get_untracked() {
                    shell.close();
                }
            }
            _ => {}
        }
    });
    let pointer = window_event_listener(ev::mousemove, move |_| tips(true));
    let focus = window_event_listener(ev::focusin, move |_| tips(true));
    on_cleanup(move || {
        keys.remove();
        pointer.remove();
        focus.remove();
    });

    // A collapsed sidebar stays collapsed on the next visit.
    Effect::new(move |_| {
        if let Some(storage) = storage() {
            let _ = if shell.collapsed.get() {
                storage.set_item(SIDEBAR_KEY, "collapsed")
            } else {
                storage.remove_item(SIDEBAR_KEY)
            };
        }
    });

    // The model list names the default model (the picker's first pick) and which models a first run pulls.
    // `?autorun=1` runs a share link once, after the list has loaded.
    let autorun = StoredValue::new(autorun_requested());
    Effect::new(move |_| {
        let Some(listed) = models.get() else {
            return;
        };
        if let Ok(list) = listed {
            deck.set_models(list);
        }
        if autorun.get_value() {
            autorun.set_value(false);
            if deck.share_error.get_untracked().is_none() {
                deck.run();
            }
        }
    });
    // A run may have pulled its model: list again.
    Effect::new(move |previous: Option<usize>| {
        let run = deck
            .last
            .with(|last| last.as_ref().map_or(0, |run| run.number));
        if previous.is_some_and(|previous| previous != run) {
            models.refetch();
        }
        run
    });

    // When the columns stack, the answers land a screen below Run: bring the first question, or the fault, into view.
    Effect::new(move |_| {
        if deck.last.with(Option::is_some) {
            request_animation_frame(scroll_to_result);
        }
    });

    view! {
        <a class="skip-link" href="#content">"Skip to the page"</a>
        <p class="visually-hidden" role="status" data-testid="run-status">
            {move || run_status(deck)}
        </p>
        <p class="visually-hidden" role="status" data-testid="notice">
            {move || deck.notice.get()}
        </p>
        <div class="shell" class:collapsed=move || shell.collapsed.get() class:drawer-open=move || shell.drawer.get()>
            <sidebar::Sidebar deck=deck shell=shell models=models />
            <div class="scrim" aria-hidden="true" on:click=move |_| shell.close()></div>
            <div class="main" inert=move || shell.drawer.get()>
                <topbar::Topbar deck=deck shell=shell />
                <main class="page" id="content" tabindex="-1">
                    <div class="page-title">
                        <div class="page-title-row">
                            <Icon name="page" class="page-icon" />
                            <h1 class="page-heading">"Ardana playground"</h1>
                        </div>
                        <p class="page-lede">
                            "Run a state and questions against a local System 1 decision model. Every figure is the "
                            "API's own, and the request is one copy away in Snippets."
                        </p>
                    </div>
                    <div class="columns">
                        <state::State deck=deck />
                        <questions::Questions deck=deck />
                    </div>
                    <exchange::Exchange deck=deck />
                    <snippets::Snippets deck=deck />
                </main>
            </div>
        </div>
    }
}

fn autorun_requested() -> bool {
    window()
        .location()
        .search()
        .ok()
        .and_then(|search| web_sys::UrlSearchParams::new_with_str(&search).ok())
        .and_then(|params| params.get("autorun"))
        .is_some_and(|value| value == "1")
}

fn storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

/// Shows or hides every tooltip, through the `tips-off` class on the root element.
fn tips(shown: bool) {
    if let Some(root) = document().document_element() {
        let list = root.class_list();
        let _ = if shown {
            list.remove_1("tips-off")
        } else {
            list.add_1("tips-off")
        };
    }
}

/// Whether the media query holds now.
pub fn matches(query: &str) -> bool {
    window()
        .match_media(query)
        .ok()
        .flatten()
        .is_some_and(|list| list.matches())
}

/// What the last run came to, for the polite status region.
fn run_status(deck: Deck) -> String {
    deck.last.with(|last| {
        let Some(run) = last else {
            return String::new();
        };
        match &run.reply {
            Ok(Reply::Answered { model, answers, .. }) => {
                let n = answers.len();
                let questions = if n == 1 { "question" } else { "questions" };
                format!(
                    "Answered by {model}: {n} {questions}, {:.0} ms",
                    run.exchange.latency_ms
                )
            }
            Ok(Reply::Failed { status, .. }) => format!("HTTP {status}, not answered"),
            Err(_) => "No response, not answered".to_string(),
        }
    })
}

/// Scrolls `element` into view, instantly under reduced motion.
pub fn scroll_to(element: &web_sys::Element) {
    let options = web_sys::ScrollIntoViewOptions::new();
    options.set_block(web_sys::ScrollLogicalPosition::Start);
    options.set_behavior(if matches("(prefers-reduced-motion: reduce)") {
        web_sys::ScrollBehavior::Instant
    } else {
        web_sys::ScrollBehavior::Smooth
    });
    element.scroll_into_view_with_scroll_into_view_options(&options);
}

/// While the columns stack, scrolls the fault or the first question to the top.
fn scroll_to_result() {
    if !matches(NARROW) {
        return;
    }
    let target = document()
        .query_selector("[data-testid='fault']")
        .ok()
        .flatten()
        .or_else(|| document().query_selector(".question").ok().flatten());
    if let Some(target) = target {
        scroll_to(&target);
    }
}

/// Moves focus to the element `id` once the view has caught up with the latest change.
pub fn focus_later(id: String) {
    request_animation_frame(move || {
        if let Some(element) = document()
            .get_element_by_id(&id)
            .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = element.focus();
        }
    });
}

/// An element id for a question id, which may hold any text: `q-` and its bytes in hex.
pub fn dom_id(id: &str) -> String {
    let hex: String = id.bytes().map(|b| format!("{b:02x}")).collect();
    format!("q-{hex}")
}

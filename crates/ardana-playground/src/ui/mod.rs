//! The playground, in ardana.ai's brand: a sidebar headed by the logo on the left, and on the right the sticky top bar
//! with Run over one page: the state on the left, the questions with their answers on the right, then the raw
//! exchange and the snippets as toggle blocks.

mod builder;
mod controls;
mod exchange;
mod figure;
mod icons;
mod logo;
mod questions;
mod sidebar;
mod snippets;
mod state;
mod topbar;

use leptos::ev;
use leptos::prelude::*;

use wasm_bindgen::JsCast;

use crate::api::{ApiClient, Place};
use crate::deck::Deck;
use crate::engine;
use crate::request::Reply;
use crate::share;

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
    // and dismisses any tooltip until the pointer or focus moves again; Tab brings a text field it reaches whole into
    // view.
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
            "Tab" => request_animation_frame(show_text_field),
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

    // The model list names the default model (the picker's first pick) and where each model runs. `?autorun=1` runs a
    // share link once, after the list has loaded (a first run in this tab waits for a tap: `Deck::autorun`).
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
                deck.autorun();
            }
        }
    });
    // Whether a run of the picked "In browser" row downloads its files first: asked of this browser's Cache Storage
    // whenever such a row is picked, and again once a run is over or stopped.
    let pending = deck.runner.pending();
    Effect::new(move |_| {
        if pending.get() || !deck.in_browser.get() {
            return;
        }
        let name = deck.model.get();
        leptos::task::spawn_local(async move {
            let kept = engine::kept(&name).await;
            if deck.in_browser.get_untracked() && deck.model.get_untracked() == name {
                deck.kept.set(Some((name, kept)));
            }
        });
    });
    // `ardana pull` or `ardana rm` may have changed the server's models since the list was fetched: list again after
    // each run, and whenever the tab comes back (the window takes the focus again, or the page is shown again after
    // another tab or a terminal), so a model pulled meanwhile is a server row that Run sends, without a reload.
    Effect::new(move |previous: Option<usize>| {
        let run = deck
            .last
            .with(|last| last.as_ref().map_or(0, |run| run.number));
        if previous.is_some_and(|previous| previous != run) {
            models.refetch();
        }
        run
    });
    let refocused = window_event_listener(ev::focus, move |_| models.refetch());
    let shown = window_event_listener(ev::visibilitychange, move |_| {
        if !document().hidden() {
            models.refetch();
        }
    });
    on_cleanup(move || {
        refocused.remove();
        shown.remove();
    });

    // When the columns stack, the answers land a screen below Run: bring the first question, or the fault, into view,
    // and the focus with it.
    Effect::new(move |_| {
        if deck.last.with(Option::is_some) {
            request_animation_frame(show_result);
        }
    });

    // Said once per change: a download moves its bar every half percent, and the status in tenths. Another run that
    // comes to the same words is said again.
    let status = Memo::new(move |_| run_status(deck));
    let status_region = NodeRef::<leptos::html::P>::new();
    Effect::new(move |said: Option<usize>| {
        let text = status.get();
        let run = deck
            .last
            .with(|last| last.as_ref().map_or(0, |run| run.number));
        if let Some(region) = status_region.get() {
            announce(&region, text, said.is_some_and(|said| said != run));
        }
        run
    });
    // Each notice is said, the same words again too (a preset loaded twice).
    let notice_region = NodeRef::<leptos::html::P>::new();
    Effect::new(move |_| {
        let text = deck.notice.get();
        if let Some(region) = notice_region.get() {
            announce(&region, text, true);
        }
    });

    view! {
        // It focuses and scrolls without the fragment, which would push a history entry: Back would then load the
        // share link in the URL again, over the editors. Under the open drawer the page is inert, and so is the link.
        <a
            class="skip-link"
            href="#content"
            inert=move || shell.drawer.get()
            on:click=move |event| {
                event.prevent_default();
                reveal("content");
            }
        >
            "Skip to the page"
        </a>
        // Rendered empty; `announce` writes what they say.
        <p class="visually-hidden" role="status" data-testid="run-status" node_ref=status_region></p>
        <p class="visually-hidden" role="status" data-testid="notice" node_ref=notice_region></p>
        <logo::LogoSymbol />
        <div class="shell" class:collapsed=move || shell.collapsed.get() class:drawer-open=move || shell.drawer.get()>
            <sidebar::Sidebar deck=deck shell=shell models=models />
            <div class="scrim" aria-hidden="true" on:click=move |_| shell.close()></div>
            <div class="main" inert=move || shell.drawer.get()>
                <topbar::Topbar deck=deck shell=shell />
                <main class="page" id="content" tabindex="-1">
                    <div class="page-title">
                        <h1 class="page-heading">"Ardana playground"</h1>
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

/// What the polite status region says: where a run in this tab is (each stage once, a download in tenths), that a run
/// was stopped, or what the last run came to.
fn run_status(deck: Deck) -> String {
    if let Some(stage) = deck.stage.get() {
        let name = deck.run_model.get().unwrap_or_default();
        let size = deck.listed(&name).and_then(|m| m.x_browser);
        if let Some(status) = topbar::tab_status(&name, size, stage) {
            return status;
        }
    }
    if deck.stopped.get() {
        return engine::stopped();
    }
    deck.last.with(|last| {
        let Some(run) = last else {
            return String::new();
        };
        match &run.reply {
            Ok(Reply::Answered { model, answers, .. }) => {
                let n = answers.len();
                let questions = if n == 1 { "question" } else { "questions" };
                format!(
                    "Answered by {model}{}: {n} {questions}, {:.0} ms",
                    place(run.exchange.place).unwrap_or_default(),
                    run.exchange.latency_ms
                )
            }
            Ok(Reply::Failed { status, .. }) => format!("HTTP {status}, not answered"),
            Err(unanswered) => match &unanswered.advice {
                Some(advice) => format!("Not answered in this tab. {advice}"),
                None => "No response, not answered".to_string(),
            },
        }
    })
}

/// Where a run was answered, after the model's name: nothing for the server, the tab and its backend for a run here.
pub fn place(place: Place) -> Option<String> {
    match place {
        Place::Server => None,
        Place::Tab(None) => Some(" in this tab".to_string()),
        Place::Tab(Some(backend)) => Some(format!(" in this tab on {}", backend.name())),
    }
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

/// Brings `block` into view and moves focus to `target` (the block, or a part of it) on the next frame, without
/// scrolling again.
fn show(block: &web_sys::Element, target: web_sys::Element) {
    scroll_to(block);
    request_animation_frame(move || {
        if let Ok(target) = target.dyn_into::<web_sys::HtmlElement>() {
            let options = web_sys::FocusOptions::new();
            options.set_prevent_scroll(true);
            let _ = target.focus_with_options(&options);
        }
    });
}

/// Brings the section `id` into view and moves focus to it: a closed toggle block opens and its summary takes focus,
/// any other section takes it itself.
pub fn reveal(id: &str) {
    let Some(element) = document().get_element_by_id(id) else {
        return;
    };
    let block = element.tag_name() == "DETAILS";
    if block {
        let _ = element.set_attribute("open", "");
    }
    let target = if block {
        element.query_selector(":scope > summary").ok().flatten()
    } else {
        None
    };
    show(&element, target.unwrap_or_else(|| element.clone()));
}

/// While the columns stack, brings the fault or the first question to the top, and the focus to its title or the
/// question's id: on a phone the answers land a screen below Run (or below the editor Ctrl+Enter was pressed in).
fn show_result() {
    if !matches(NARROW) {
        return;
    }
    let found = |block: &str, title: &str| {
        let block = document().query_selector(block).ok().flatten()?;
        let title = block.query_selector(title).ok().flatten()?;
        Some((block, title))
    };
    if let Some((block, title)) = found("[data-testid='fault']", ".fault-title")
        .or_else(|| found(".question", ".question-id"))
    {
        show(&block, title);
    }
}

/// Writes `text` into the live region `region`. With `again`, words the region holds already are said once more: the
/// region is emptied for a frame first, so assistive technology hears a change; a newer text in the meantime wins.
fn announce(region: &web_sys::HtmlElement, text: String, again: bool) {
    let held = region.text_content().unwrap_or_default();
    if again && !text.is_empty() && held == text {
        region.set_text_content(None);
        let region = region.clone();
        request_animation_frame(move || {
            if region.text_content().unwrap_or_default().is_empty() {
                region.set_text_content(Some(&text));
            }
        });
    } else if held != text {
        region.set_text_content(Some(&text));
    }
}

/// Scrolls a text field that has just taken the focus whole into view, under the sticky bars: a browser scrolls only
/// the field's caret into view, which can leave the field's top under them (WCAG 2.4.11). A field taller than the room
/// below the bars (the root's scroll padding) keeps the browser's place, its caret in view.
fn show_text_field() {
    let Some(field) = document()
        .active_element()
        .filter(|field| field.tag_name() == "TEXTAREA")
    else {
        return;
    };
    let covered = document()
        .document_element()
        .and_then(|root| window().get_computed_style(&root).ok().flatten())
        .and_then(|style| style.get_property_value("scroll-padding-top").ok())
        .and_then(|padding| padding.trim_end_matches("px").parse::<f64>().ok())
        .unwrap_or_default();
    let room = window()
        .inner_height()
        .ok()
        .and_then(|height| height.as_f64())
        .unwrap_or_default()
        - covered;
    if field.get_bounding_client_rect().height() <= room {
        let options = web_sys::ScrollIntoViewOptions::new();
        options.set_block(web_sys::ScrollLogicalPosition::Nearest);
        field.scroll_into_view_with_scroll_into_view_options(&options);
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

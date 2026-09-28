//! The fascia: the top rail (model, counters, RUN), the console strip (presets, share link), the state cassette on
//! the left, the question channels on the right, and the raw exchange and snippets below.

mod builder;
mod cassette;
mod channels;
mod console;
mod controls;
mod exchange;
mod figure;
mod rail;
mod snippets;

use leptos::ev;
use leptos::prelude::*;

use wasm_bindgen::JsCast;

use crate::api::ApiClient;
use crate::deck::Deck;
use crate::request::Reply;
use crate::share;

#[component]
pub fn App() -> impl IntoView {
    let client = ApiClient::same_origin();
    let deck = Deck::new(client.clone());
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

    // `?autorun=1` runs a share link once, after the model list has loaded.
    let autorun = StoredValue::new(autorun_requested());
    Effect::new(move |_| {
        if models.get().is_some() && autorun.get_value() {
            autorun.set_value(false);
            if deck.share_error.get_untracked().is_none() {
                deck.run();
            }
        }
    });

    // On a phone the answers land a screen below RUN: bring the first channel, or the fault, into view.
    Effect::new(move |_| {
        if deck.last.with(Option::is_some) {
            request_animation_frame(scroll_to_result);
        }
    });

    view! {
        <p class="visually-hidden" role="status" data-testid="run-status">
            {move || run_status(deck)}
        </p>
        <p class="visually-hidden" role="status" data-testid="notice">
            {move || deck.notice.get()}
        </p>
        <div class="fascia">
            <rail::Rail deck=deck models=models />
            <main class="deck">
                <console::Console deck=deck />
                <cassette::Cassette deck=deck />
                <channels::Channels deck=deck />
                <exchange::Exchange deck=deck />
                <snippets::Snippets deck=deck />
            </main>
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

/// Ctrl+Enter or Cmd+Enter in an editor presses RUN.
pub fn run_shortcut(deck: Deck) -> impl Fn(ev::KeyboardEvent) + Copy + 'static {
    move |event: ev::KeyboardEvent| {
        if event.key() == "Enter" && (event.ctrl_key() || event.meta_key()) {
            event.prevent_default();
            deck.run();
        }
    }
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

/// Below 720 px, scrolls the fault or the first channel to the top (instantly under reduced motion).
fn scroll_to_result() {
    let matches = |query: &str| {
        window()
            .match_media(query)
            .ok()
            .flatten()
            .is_some_and(|list| list.matches())
    };
    if !matches("(max-width: 720px)") {
        return;
    }
    let target = document()
        .query_selector(".fault")
        .ok()
        .flatten()
        .or_else(|| document().query_selector(".channel").ok().flatten());
    if let Some(target) = target {
        let options = web_sys::ScrollIntoViewOptions::new();
        options.set_block(web_sys::ScrollLogicalPosition::Start);
        options.set_behavior(if matches("(prefers-reduced-motion: reduce)") {
            web_sys::ScrollBehavior::Instant
        } else {
            web_sys::ScrollBehavior::Smooth
        });
        target.scroll_into_view_with_scroll_into_view_options(&options);
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

//! The fascia: the top rail (model, counters, RUN), the state cassette on the left, the question channels on the
//! right, and the raw exchange below.

mod cassette;
mod channels;
mod exchange;
mod figure;
mod rail;

use leptos::ev;
use leptos::prelude::*;

use crate::api::ApiClient;
use crate::deck::Deck;
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

    view! {
        <div class="fascia">
            <rail::Rail deck=deck models=models />
            <main class="deck">
                <cassette::Cassette deck=deck />
                <channels::Channels deck=deck />
                <exchange::Exchange deck=deck />
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

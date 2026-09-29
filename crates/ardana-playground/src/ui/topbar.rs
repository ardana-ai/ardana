//! The sticky top bar: the sidebar opener, the page's name, the stale tag, Share with its popover, and the blue Run.
//! Under it, the banner that says why Run is held or what a first run is downloading.

use ardana_api::human_size;
use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::controls::CopyKey;
use super::icons::Icon;
use super::{Shell, focus_later};
use crate::deck::Deck;
use crate::share::{self, SharePayload};

#[component]
pub fn Topbar(deck: Deck, shell: Shell) -> impl IntoView {
    let pending = deck.runner.pending();
    // The run in flight names a model the server pulls first. `/v1/models` gives no progress, so the page says what
    // is downloading and how much, not how far along it is.
    let pulling = move || {
        if !pending.get() {
            return None;
        }
        let name = deck.run_model.get()?;
        deck.unpulled(&name).map(|model| match model.x_size {
            Some(bytes) => format!("Downloading {name} ({}) on first run", human_size(bytes)),
            None => format!("Downloading {name} on first run"),
        })
    };
    view! {
        <header class="topbar">
            <button
                type="button"
                id="sidebar-opener"
                class="button button-icon opener tip tip-below tip-start"
                aria-label="Open sidebar"
                aria-controls="sidebar"
                aria-expanded=move || shell.drawer.get().to_string()
                data-tip="Open sidebar\nCtrl+\\ or ⌘\\"
                on:click=move |_| shell.open()
            >
                <Icon name="chevrons-right" />
            </button>
            <div class="topbar-crumb">
                <Icon name="page" />
                <span class="topbar-crumb-text">"Playground"</span>
            </div>
            <div class="topbar-actions">
                <p class="changed" class:on=move || deck.stale.get() data-testid="changed">
                    {move || {
                        deck.stale
                            .get()
                            .then(|| view! { <span class="tag">"Inputs changed"</span> })
                    }}
                </p>
                <Share deck=deck />
                // `aria-disabled`, not `disabled`: a button that disables itself while focused drops focus to the page.
                <button
                    type="button"
                    class="button button-primary run-key tip tip-below tip-end"
                    aria-keyshortcuts="Control+Enter Meta+Enter"
                    aria-busy=move || pending.get().to_string()
                    aria-disabled=move || (!deck.can_run()).to_string()
                    aria-describedby="run-note"
                    data-tip="Send the state and questions\nCtrl+Enter or ⌘↵"
                    on:click=move |_| deck.run()
                >
                    <Show when=move || pending.get()>
                        <Icon name="spinner" class="spinner icon-sm" />
                    </Show>
                    <span class="run-label" data-testid="run-label">
                        {move || match (pending.get(), pulling().is_some()) {
                            (true, true) => "Pulling",
                            (true, false) => "Running",
                            _ => "Run",
                        }}
                    </span>
                </button>
            </div>
        </header>
        // A wait or a hint is quiet; only an error takes the fault colours.
        <p
            id="run-note"
            class="banner"
            class:quiet=move || pulling().is_some() || deck.questions_error.with(Option::is_none)
            data-testid="run-note"
        >
            {move || pulling().or_else(|| deck.held_reason().map(str::to_string))}
        </p>
    }
}

/// Share: a link (Jev's link format) to the editors and the picked model, in a popover under the button.
#[component]
fn Share(deck: Deck) -> impl IntoView {
    let open = RwSignal::new(false);
    let origin = window().location().origin().unwrap_or_default();
    let link = move || {
        let payload = SharePayload::new(
            deck.state_text.get(),
            deck.questions_text.get(),
            deck.model.get(),
        );
        share::link(&origin, &payload)
    };
    let link = Memo::new(move |_| open.get().then(&link).unwrap_or_default());
    // A click anywhere outside the popover and its button closes it, as Notion's popovers do.
    let outside = window_event_listener(ev::click, move |event| {
        if !open.get_untracked() {
            return;
        }
        let inside = document().get_element_by_id("share").is_some_and(|share| {
            event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Node>().ok())
                .is_some_and(|node| share.contains(Some(&node)))
        });
        if !inside {
            open.set(false);
        }
    });
    on_cleanup(move || outside.remove());
    let toggle = move |_| {
        let now_open = !open.get_untracked();
        open.set(now_open);
        if now_open {
            focus_later("share-link".to_string());
        }
    };
    view! {
        <div
            class="share"
            id="share"
            on:keydown=move |event| {
                if event.key() == "Escape" && open.get_untracked() {
                    open.set(false);
                    focus_later("share-button".to_string());
                }
            }
        >
            <button
                type="button"
                id="share-button"
                class="button"
                aria-label="Share link"
                aria-expanded=move || open.get().to_string()
                aria-controls="share-panel"
                on:click=toggle
            >
                <Icon name="link" />
                "Share"
            </button>
            <Show when=move || open.get()>
                <div class="popover" id="share-panel">
                    <label class="field-label" for="share-link">"Link to these inputs"</label>
                    <div class="popover-row">
                        <input
                            id="share-link"
                            class="field-input field-code"
                            type="text"
                            readonly
                            spellcheck="false"
                            prop:value=move || link.get()
                            on:focus=move |event| {
                                if let Some(input) = event
                                    .target()
                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                {
                                    input.select();
                                }
                            }
                        />
                        <CopyKey what="share link" text=move || link.get() />
                    </div>
                    <p class="field-note">"Anyone with this link opens these inputs and this model on their own Ardana."</p>
                </div>
            </Show>
        </div>
    }
}

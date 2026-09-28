//! The console strip above the deck: the preset keys, which load a state and its questions, and the share key, which
//! opens a Jev share link of the editors and the picked model.

use leptos::prelude::*;

use super::controls::CopyKey;
use super::focus_later;
use crate::deck::Deck;
use crate::presets::PRESETS;
use crate::share::{self, SharePayload};

#[component]
pub fn Console(deck: Deck) -> impl IntoView {
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
    view! {
        <div class="console">
            <div class="presets" role="group" aria-labelledby="presets-legend">
                <span class="legend" id="presets-legend">"Presets"</span>
                <span class="preset-keys">
                    {PRESETS
                        .iter()
                        .map(|preset| {
                            view! {
                                <button
                                    type="button"
                                    class="plate-key"
                                    on:click=move |_| {
                                        let (state, questions) = preset.texts();
                                        deck.load_preset(state, questions);
                                    }
                                >
                                    {preset.name}
                                </button>
                            }
                        })
                        .collect_view()}
                </span>
                <Show when=move || deck.previous.with(Option::is_some)>
                    <button
                        type="button"
                        class="plate-key"
                        on:click=move |_| {
                            deck.restore_previous();
                            focus_later("state".to_string());
                        }
                    >
                        "Restore previous"
                    </button>
                </Show>
            </div>
            <div class="share">
                <button
                    type="button"
                    class="plate-key"
                    aria-expanded=move || open.get().to_string()
                    aria-controls="share-panel"
                    on:click=move |_| open.update(|o| *o = !*o)
                >
                    "Share link"
                </button>
            </div>
            <Show when=move || open.get()>
                <div class="share-panel" id="share-panel">
                    <label class="legend" for="share-link">
                        "Jev share link"
                    </label>
                    <div class="share-row">
                        <input
                            id="share-link"
                            class="field-input share-link"
                            type="text"
                            readonly
                            spellcheck="false"
                            prop:value=move || link.get()
                            on:focus=move |event| {
                                use wasm_bindgen::JsCast;
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
                </div>
            </Show>
        </div>
    }
}

//! The state cassette: a real text area behind the cassette window, the input-token counter of the last run, and
//! how the text will be sent.

use leptos::prelude::*;
use serde_json::Value;

use super::rail::{Counter, usage};
use super::run_shortcut;
use crate::deck::Deck;
use crate::request::state_value;

#[component]
pub fn Cassette(deck: Deck) -> impl IntoView {
    let sent_as_json = Memo::new(move |_| {
        deck.state_text
            .with(|text| !matches!(state_value(text), Value::String(_)))
    });
    view! {
        <section class="cassette" aria-labelledby="state-title">
            <div class="cassette-label">
                <h2 id="state-title" class="engraved">
                    <label for="state">"State"</label>
                </h2>
                <p class="mode-lamps">
                    <span class="visually-hidden">
                        {move || if sent_as_json.get() { "Sent as JSON" } else { "Sent as text" }}
                    </span>
                    <span class="mode-lamp" class:on=move || !sent_as_json.get() aria-hidden="true">
                        <span class="lamp"></span>
                        "Text"
                    </span>
                    <span class="mode-lamp" class:on=move || sent_as_json.get() aria-hidden="true">
                        <span class="lamp"></span>
                        "JSON"
                    </span>
                </p>
            </div>
            <div class="cassette-window">
                <textarea
                    id="state"
                    class="tape"
                    spellcheck="false"
                    placeholder="Paste the state: a ticket, a transcript, a resume or a JSON record."
                    prop:value=move || deck.state_text.get()
                    on:input=move |event| deck.state_text.set(event_target_value(&event))
                    on:keydown=run_shortcut(deck)
                ></textarea>
            </div>
            <div class="cassette-foot">
                <Counter
                    label="Input tokens"
                    testid="cassette-tokens"
                    value=move || usage(deck, |i, _| i, "input_tokens")
                />
                {move || {
                    deck.share_error
                        .get()
                        .map(|err| {
                            view! {
                                <p class="fault-line" role="alert" data-testid="share-error">
                                    "This share link could not be loaded: "
                                    {err}
                                </p>
                            }
                        })
                }}
            </div>
        </section>
    }
}

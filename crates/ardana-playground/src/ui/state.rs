//! The state block: a real text area on a code block, the tag saying how the text will be sent, and the input-token
//! count of the last run.

use leptos::prelude::*;
use serde_json::Value;

use super::questions::usage;
use crate::deck::Deck;
use crate::request::state_value;

#[component]
pub fn State(deck: Deck) -> impl IntoView {
    let sent_as_json = Memo::new(move |_| {
        deck.state_text
            .with(|text| !matches!(state_value(text), Value::String(_)))
    });
    view! {
        <section class="state" aria-labelledby="state-title">
            <div class="block-head">
                <h2 id="state-title" class="block-heading">
                    <label for="state">"State"</label>
                </h2>
                <p class="state-meta">
                    <span class="tag" data-testid="state-mode">
                        {move || if sent_as_json.get() { "Sent as JSON" } else { "Sent as text" }}
                    </span>
                </p>
            </div>
            <textarea
                id="state"
                class="code-block state-text"
                spellcheck="false"
                placeholder="Paste the state: a ticket, a transcript, a resume or a JSON record."
                prop:value=move || deck.state_text.get()
                on:input=move |event| deck.set_state_text(event_target_value(&event))
            ></textarea>
            <div class="state-foot">
                {move || {
                    usage(deck, |i, _| i, "input_tokens")
                        .map(|tokens| {
                            view! {
                                <span data-testid="state-tokens">
                                    {tokens}
                                    " input tokens in the last run"
                                </span>
                            }
                        })
                }}
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

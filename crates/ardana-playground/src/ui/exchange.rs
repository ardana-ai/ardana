//! The raw exchange: the exact JSON the last run sent, and the exact body it received with its status.

use leptos::prelude::*;

use crate::deck::Deck;

#[component]
pub fn Exchange(deck: Deck) -> impl IntoView {
    let sent = move || {
        deck.last
            .with(|last| last.as_ref().map(|run| run.exchange.request.clone()))
    };
    let received = move || {
        deck.last.with(|last| {
            last.as_ref().map(|run| match &run.exchange.response {
                Ok((status, body)) => (format!("HTTP {status}"), body.clone()),
                Err(err) => ("No response".to_string(), err.clone()),
            })
        })
    };
    view! {
        <section class="exchange" aria-labelledby="exchange-title">
            <div class="panel-head">
                <h2 id="exchange-title" class="engraved">"Raw exchange"</h2>
                <p class="legend">"Last run, byte for byte"</p>
            </div>
            <div class="exchange-grid">
                <div class="exchange-side">
                    <h3 class="legend">"Sent · POST /v1/systemone"</h3>
                    <Show
                        when=move || sent().is_some()
                        fallback=|| {
                            view! {
                                <p class="wire-empty">
                                    "Nothing sent yet. Press Run to send the state and questions above."
                                </p>
                            }
                        }
                    >
                        <pre class="wire" tabindex="0" data-testid="raw-request" aria-label="Request sent">
                            {move || sent().unwrap_or_default()}
                        </pre>
                    </Show>
                </div>
                <div class="exchange-side">
                    <h3 class="legend">
                        "Received"
                        {move || received().map(|(status, _)| view! { " · " <span data-testid="raw-status">{status}</span> })}
                    </h3>
                    <Show
                        when=move || received().is_some()
                        fallback=|| view! { <p class="wire-empty">"The response body appears here, byte for byte."</p> }
                    >
                        <pre class="wire" tabindex="0" data-testid="raw-response" aria-label="Response received">
                            {move || received().map(|(_, body)| body).unwrap_or_default()}
                        </pre>
                    </Show>
                </div>
            </div>
        </section>
    }
}

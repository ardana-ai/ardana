//! The raw exchange: a toggle block holding the exact JSON the last run sent and the exact body it received, with
//! its status. A run in this tab shows the request it answered and the body the API answers it with.

use leptos::prelude::*;

use super::icons::Icon;
use crate::deck::{Deck, Runs};

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
                Err(unanswered) => ("No response".to_string(), unanswered.reason.clone()),
            })
        })
    };
    view! {
        <details class="toggle" id="exchange" open=true>
            <summary>
                <span class="toggle-marker">
                    <Icon name="chevron-right" />
                </span>
                <h2 class="toggle-heading">"Raw exchange"</h2>
                <span class="toggle-caption">"Last run, byte for byte"</span>
            </summary>
            <div class="exchange-grid">
                <div class="exchange-side">
                    // Where the last run went, or before one, where Run would send (nowhere while the server lacks
                    // the model).
                    <h3>
                        {move || {
                            let runs = deck.last.with(|last| match last {
                                Some(run) if run.in_tab() => Runs::Tab,
                                Some(_) => Runs::Server,
                                None => deck.runs(),
                            });
                            match runs {
                                Runs::Server => "Sent · POST /v1/systemone",
                                Runs::Tab => "Sent · in this tab",
                                Runs::Cli => "Sent",
                            }
                        }}
                    </h3>
                    <Show
                        when=move || sent().is_some()
                        fallback=move || {
                            view! {
                                <p class="wire-empty">
                                    {move || match deck.runs() {
                                        Runs::Cli => "Nothing sent yet.",
                                        _ => "Nothing sent yet. Press Run to send the state and questions above.",
                                    }}
                                </p>
                            }
                        }
                    >
                        <pre class="code-block wire" tabindex="0" role="group" data-testid="raw-request" aria-label="Request sent">
                            {move || sent().unwrap_or_default()}
                        </pre>
                    </Show>
                </div>
                <div class="exchange-side">
                    <h3>
                        "Received"
                        {move || received().map(|(status, _)| view! { " · " <span data-testid="raw-status">{status}</span> })}
                    </h3>
                    <Show
                        when=move || received().is_some()
                        fallback=|| view! { <p class="wire-empty">"The response body appears here, byte for byte."</p> }
                    >
                        <pre class="code-block wire" tabindex="0" role="group" data-testid="raw-response" aria-label="Response received">
                            {move || received().map(|(_, body)| body).unwrap_or_default()}
                        </pre>
                    </Show>
                </div>
            </div>
        </details>
    }
}

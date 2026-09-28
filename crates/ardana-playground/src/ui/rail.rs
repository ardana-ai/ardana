//! The top rail: maker's plate, the model switch, the amber counters and the RUN transport key.

use ardana_api::ModelsResponse;
use leptos::prelude::*;

use super::figure::Verbatim;
use crate::deck::{DEFAULT_MODEL, Deck};
use crate::request::Reply;

#[component]
pub fn Rail(deck: Deck, models: LocalResource<Result<ModelsResponse, String>>) -> impl IntoView {
    let pending = deck.runner.pending();
    view! {
        <header class="rail">
            <div class="maker">
                <h1 class="maker-name">"Ardana"</h1>
                <p class="maker-line">"System 1 playground"</p>
            </div>
            <ModelSwitch deck=deck models=models />
            <div class="counters" role="group" aria-label="Last run">
                <Counter
                    label="Latency ms"
                    testid="latency"
                    value=move || {
                        deck.last.with(|last| {
                            last.as_ref().map(|run| {
                                let ms = run.exchange.latency_ms;
                                view! {
                                    <span class="matrix-digits" data-ms=ms.to_string()>
                                        {format!("{ms:.0}")}
                                    </span>
                                }
                                .into_any()
                            })
                        })
                    }
                />
                <Counter
                    label="Input tokens"
                    testid="tokens-in"
                    value=move || usage(deck, |i, _| i, "input_tokens")
                />
                <Counter
                    label="Output tokens"
                    testid="tokens-out"
                    value=move || usage(deck, |_, o| o, "output_tokens")
                />
            </div>
            <button
                type="button"
                class="run-key"
                aria-keyshortcuts="Control+Enter Meta+Enter"
                aria-busy=move || pending.get().to_string()
                disabled=move || !deck.can_run()
                on:click=move |_| deck.run()
            >
                <span class="run-led" aria-hidden="true"></span>
                <svg class="run-glyph" viewBox="0 0 16 16" aria-hidden="true">
                    <path d="M4 2.5v11l9-5.5z" />
                </svg>
                <span class="run-legend">{move || if pending.get() { "Running" } else { "Run" }}</span>
            </button>
        </header>
    }
}

/// One of the last run's token counts, from `usage`.
pub fn usage(
    deck: Deck,
    pick: fn(Option<u64>, Option<u64>) -> Option<u64>,
    key: &'static str,
) -> Option<AnyView> {
    deck.last
        .with(|last| match last.as_ref().map(|run| &run.reply) {
            Some(Ok(Reply::Answered {
                input_tokens,
                output_tokens,
                ..
            })) => pick(*input_tokens, *output_tokens).map(|n| {
                view! {
                    <Verbatim
                        field=format!("/usage/{key}")
                        value=n.to_string()
                        class="matrix-digits"
                    />
                }
                .into_any()
            }),
            _ => None,
        })
}

/// A dot-matrix window with its engraved legend; dashes until there is a figure.
#[component]
pub fn Counter(
    label: &'static str,
    testid: &'static str,
    value: impl Fn() -> Option<AnyView> + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <div class="counter" data-testid=testid>
            <div class="matrix">
                {move || {
                    value()
                        .unwrap_or_else(|| {
                            view! {
                                <span class="matrix-idle">
                                    <span aria-hidden="true">"\u{a0}"</span>
                                    <span class="visually-hidden">"none yet"</span>
                                </span>
                            }
                            .into_any()
                        })
                }}
            </div>
            <span class="legend">{label}</span>
        </div>
    }
}

#[component]
fn ModelSwitch(deck: Deck, models: LocalResource<Result<ModelsResponse, String>>) -> impl IntoView {
    let names = move || -> Vec<String> {
        let mut names = vec![DEFAULT_MODEL.to_string()];
        if let Some(Ok(list)) = models.get() {
            names.extend(list.models.into_iter().map(|m| m.name));
        }
        let picked = deck.model.get();
        if !names.contains(&picked) {
            names.push(picked);
        }
        names
    };
    let failure = move || match models.get() {
        Some(Err(err)) => Some(view! { <p class="fault-line" role="alert">{err}</p> }),
        _ => None,
    };
    view! {
        <div class="switch">
            <div class="switch-plate">
                <select
                    id="model"
                    class="switch-select"
                    prop:value=move || deck.model.get()
                    on:change=move |event| deck.model.set(event_target_value(&event))
                >
                    {move || {
                        names()
                            .into_iter()
                            .map(|name| {
                                let label = if name == DEFAULT_MODEL {
                                    format!("{name} (default model)")
                                } else {
                                    name.clone()
                                };
                                let selected = deck.model.get_untracked() == name;
                                view! { <option value=name selected=selected>{label}</option> }
                            })
                            .collect_view()
                    }}
                </select>
            </div>
            <label class="legend" for="model">"Model"</label>
            {failure}
            {move || {
                deck.last
                    .with(|last| match last.as_ref().map(|run| &run.reply) {
                        Some(Ok(Reply::Answered { model, .. })) => Some(model.clone()),
                        _ => None,
                    })
                    .map(|model| {
                        view! {
                            <p class="answered" data-testid="answered-by">
                                <span class="legend">"Answered by"</span>
                                <Verbatim field="/model".to_string() value=model class="answered-model" />
                            </p>
                        }
                    })
            }}
        </div>
    }
}

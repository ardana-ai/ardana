//! The top rail: maker's plate, the model switch, the amber counters and the RUN transport key.

use ardana_api::{ModelsResponse, human_size};
use leptos::prelude::*;

use super::figure::Verbatim;
use crate::deck::Deck;
use crate::request::Reply;

#[component]
pub fn Rail(deck: Deck, models: LocalResource<Result<ModelsResponse, String>>) -> impl IntoView {
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
            <div class="transport">
                // `aria-disabled`, not `disabled`: a key that disables itself while focused drops focus to the page.
                <button
                    type="button"
                    class="run-key"
                    aria-keyshortcuts="Control+Enter Meta+Enter"
                    aria-busy=move || pending.get().to_string()
                    aria-disabled=move || (!deck.can_run()).to_string()
                    aria-describedby="run-note"
                    on:click=move |_| deck.run()
                >
                    <span class="run-led" aria-hidden="true"></span>
                    <svg class="run-glyph" viewBox="0 0 16 16" aria-hidden="true">
                        <path d="M4 2.5v11l9-5.5z" />
                    </svg>
                    <span class="run-legend">
                        {move || match (pending.get(), pulling().is_some()) {
                            (true, true) => "Pulling",
                            (true, false) => "Running",
                            _ => "Run",
                        }}
                    </span>
                </button>
                <p class="changed" class:on=move || deck.stale.get() data-testid="changed">
                    <span class="lamp" aria-hidden="true"></span>
                    <span class="legend">"Changed"</span>
                    <span class="visually-hidden">
                        {move || {
                            if deck.stale.get() {
                                ": the inputs differ from the last run"
                            } else {
                                ": nothing since the last run"
                            }
                        }}
                    </span>
                </p>
            </div>
            <p id="run-note" class="run-note" class:pulling=move || pulling().is_some() data-testid="run-note">
                {move || {
                    pulling()
                        .or_else(|| {
                            deck.questions_error
                                .with(Option::is_some)
                                .then(|| "Questions JSON has an error".to_string())
                        })
                }}
            </p>
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
    let failure = move || match models.get() {
        Some(Err(err)) => Some(view! { <p class="fault-line" role="alert">{err}</p> }),
        _ => None,
    };
    // Pulled models first, then the library models a first run pulls, marked with their download size. A picked
    // model the list does not name (a share link's) is offered as it is.
    let options = move || {
        let picked = deck.model.get();
        let option = move |name: String, label: String| {
            let selected = deck.model.get_untracked() == name;
            view! { <option value=name selected=selected>{label}</option> }
        };
        deck.models.with(|models| {
            let pulled: Vec<_> = models
                .iter()
                .filter(|m| m.pulled())
                .map(|m| option(m.name.clone(), m.name.clone()))
                .collect();
            let library: Vec<_> = models
                .iter()
                .filter(|m| !m.pulled())
                .map(|m| {
                    let label = match m.x_size {
                        Some(bytes) => format!("{} · {}", m.name, human_size(bytes)),
                        None => m.name.clone(),
                    };
                    option(m.name.clone(), label)
                })
                .collect();
            let unlisted = (!picked.is_empty() && !models.iter().any(|m| m.name == picked))
                .then(|| option(picked.clone(), picked.clone()));
            view! {
                {(!pulled.is_empty()).then(|| view! { <optgroup label="Pulled">{pulled}</optgroup> })}
                {(!library.is_empty())
                    .then(|| view! { <optgroup label="Library · pulls on first run">{library}</optgroup> })}
                {unlisted}
            }
        })
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
                    {options}
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
                                {move || {
                                    deck.stale
                                        .get()
                                        .then(|| {
                                            view! {
                                                <span class="stale-note" data-testid="stale">
                                                    "From last run · inputs changed"
                                                </span>
                                            }
                                        })
                                }}
                            </p>
                        }
                    })
            }}
        </div>
    }
}

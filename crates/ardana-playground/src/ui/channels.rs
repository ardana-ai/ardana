//! The channels: one module per question, with its engraved id, its type toggle, its builder (opened by its Edit
//! key) and an amber ladder per option that climbs to the probability the API returned. Below them, the
//! `questions` JSON the channels are read from.

use ardana_api::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer};
use leptos::prelude::*;
use serde_json::Value;

use super::builder::Program;
use super::controls::Toggle;
use super::figure::{Format, Number, Verbatim, pointer};
use super::{dom_id, run_shortcut};
use crate::builder::{self, KINDS, kind_name};
use crate::deck::Deck;
use crate::request::{Detail, Reply, Shown};

const EXAMPLE: &str = r#"Example:
{
  "refund": {
    "type": "noul",
    "instructions": "Does the customer ask for a refund?"
  }
}"#;

#[component]
pub fn Channels(deck: Deck) -> impl IntoView {
    let ids = move || {
        deck.questions
            .with(|q| q.keys().cloned().collect::<Vec<_>>())
    };
    let add = move |_| {
        let mut added = None;
        let _ = deck.edit(|questions| {
            added = Some(builder::add_question(questions));
            Ok(())
        });
        deck.editing.set(added);
    };
    view! {
        <section class="channels" aria-labelledby="channels-title">
            <div class="panel-head">
                <h2 id="channels-title" class="engraved">"Questions"</h2>
                <p class="legend">"One channel per question"</p>
            </div>
            <Fault deck=deck />
            <Show
                when=move || deck.questions.with(|q| !q.is_empty())
                fallback=|| {
                    view! {
                        <div class="channels-empty">
                            <p>
                                "No channels yet. Add a question and set its type: " <code>"noul"</code>
                                " for a yes/no probability, " <code>"choice"</code> " for named options, "
                                <code>"score"</code>
                                " for ordered levels. Or load a preset, write the questions JSON below, or open a share link."
                            </p>
                        </div>
                    }
                }
            >
                <ol class="channel-list">
                    <For each=ids key=|id| id.clone() children=move |id| view! { <Channel deck=deck id=id /> } />
                </ol>
            </Show>
            <div class="channels-foot">
                <button type="button" id="add-question" class="plate-key" on:click=add>
                    "Add question"
                </button>
            </div>
            <div class="program">
                <label class="legend program-legend" for="questions">"Questions JSON"</label>
                <textarea
                    id="questions"
                    class="program-text"
                    spellcheck="false"
                    placeholder=EXAMPLE
                    aria-invalid=move || deck.questions_error.with(Option::is_some).to_string()
                    aria-describedby="questions-error"
                    prop:value=move || deck.questions_text.get()
                    on:input=move |event| deck.set_questions_text(event_target_value(&event))
                    on:keydown=run_shortcut(deck)
                ></textarea>
                <p id="questions-error" class="fault-line">
                    {move || deck.questions_error.get()}
                </p>
            </div>
        </section>
    }
}

/// The last run's error, as the API reported it.
#[component]
fn Fault(deck: Deck) -> impl IntoView {
    move || {
        deck.last.with(|last| {
            let run = last.as_ref()?;
            let (title, body) = match &run.reply {
                Ok(Reply::Answered { .. }) => return None,
                Err(err) => ("No response".to_string(), view! { <p>{err.clone()}</p> }.into_any()),
                Ok(Reply::Failed { status, detail }) => {
                    let body = match detail {
                        Detail::Validation(issues) => view! {
                            <ul class="fault-issues">
                                {issues
                                    .iter()
                                    .map(|issue| {
                                        view! {
                                            <li data-testid="fault-issue">
                                                <code class="fault-loc">{issue.loc.join(" › ")}</code>
                                                <span class="fault-msg">{issue.msg.clone()}</span>
                                            </li>
                                        }
                                    })
                                    .collect_view()}
                            </ul>
                        }
                        .into_any(),
                        Detail::Error { error_type, message } => view! {
                            <p>
                                <code class="fault-loc">{error_type.clone()}</code>
                                <span class="fault-msg" data-testid="fault-message">{message.clone()}</span>
                            </p>
                        }
                        .into_any(),
                        Detail::Other(text) => view! { <pre class="fault-raw">{text.clone()}</pre> }.into_any(),
                    };
                    (format!("HTTP {status}"), body)
                }
            };
            Some(view! {
                <div class="fault" data-testid="fault">
                    <p class="fault-title">
                        <span class="fault-lamp" aria-hidden="true"></span>
                        {title}
                        " · the request was not answered."
                    </p>
                    {body}
                </div>
            })
        })
    }
}

#[component]
fn Channel(deck: Deck, id: String) -> impl IntoView {
    let key = StoredValue::new(id.clone());
    let spec = Memo::new(move |_| {
        key.with_value(|id| {
            deck.questions
                .with(|q| q.get(id).cloned().unwrap_or(Value::Null))
        })
    });
    // This question's answer in the last run, and the run's number, so each run's ladders are drawn afresh.
    let shown = Memo::new(move |_| {
        deck.last.with(|last| match last {
            Some(run) => {
                let answer = match &run.reply {
                    Ok(Reply::Answered { answers, .. }) => {
                        key.with_value(|id| answers.get(id).cloned())
                    }
                    _ => None,
                };
                (answer, run.number)
            }
            None => (None, 0),
        })
    });
    let kind = Memo::new(move |_| spec.with(builder::kind));
    let open = Memo::new(move |_| {
        key.with_value(|id| deck.editing.with(|e| e.as_deref() == Some(id.as_str())))
    });
    let dom = dom_id(&id);
    let instructions = move || match spec.with(|s| s["instructions"].clone()) {
        Value::Null => None,
        Value::String(text) => Some(text),
        other => Some(other.to_string()),
    };
    let pick = move |kind| key.with_value(|id| deck.switch_kind(id, kind));
    // Whether this channel's answer came from a request whose state, model or question differs from now.
    let changed = Memo::new(move |_| {
        deck.inputs_changed.get()
            || deck.sent.with(|sent| {
                sent.as_ref().is_some_and(|sent| {
                    key.with_value(|id| spec.with(|spec| sent.questions.get(id) != Some(spec)))
                })
            })
    });
    let stale = Memo::new(move |_| shown.with(|(answer, _)| answer.is_some()) && changed.get());
    // The last run's 422 issues that name this question, while the question is as it was sent.
    let faults = Memo::new(move |_| {
        if changed.get() {
            return Vec::new();
        }
        deck.last
            .with(|last| match last.as_ref().map(|run| &run.reply) {
                Some(Ok(Reply::Failed {
                    detail: Detail::Validation(issues),
                    ..
                })) => key.with_value(|id| {
                    issues
                        .iter()
                        .filter(|issue| {
                            issue.loc.len() >= 3
                                && issue.loc[0] == "body"
                                && issue.loc[1] == "questions"
                                && issue.loc[2] == *id
                        })
                        .map(|issue| issue.msg.clone())
                        .collect()
                }),
                _ => Vec::new(),
            })
    });
    let toggle_edit = move |_| {
        deck.editing
            .set((!open.get_untracked()).then(|| key.get_value()));
    };
    let answer_id = id.clone();
    let edit_id = format!("{dom}-edit");
    view! {
        <li
            class="channel"
            class:editing=open
            class:stale=move || stale.get()
            class:faulted=move || faults.with(|f| !f.is_empty())
            data-testid="channel"
            data-question=id.clone()
        >
            <div class="channel-head">
                <div class="channel-title">
                    <h3 class="channel-id">{id.clone()}</h3>
                    {move || instructions().map(|text| view! { <p class="channel-instructions">{text}</p> })}
                    {move || {
                        stale
                            .get()
                            .then(|| {
                                view! {
                                    <p class="stale-note" data-testid="stale">
                                        "From last run · inputs changed"
                                    </p>
                                }
                            })
                    }}
                </div>
                <div class="channel-controls">
                    <Toggle
                        legend="Type"
                        group=format!("{dom}-type")
                        options=KINDS.map(|k| (k, kind_name(k))).to_vec()
                        checked=move |k| kind.get() == Some(k)
                        pick=pick
                        testid="type"
                    />
                    <button
                        type="button"
                        id=edit_id
                        class="plate-key edit-key"
                        aria-expanded=move || open.get().to_string()
                        aria-controls=format!("{dom}-program")
                        aria-label=format!("Edit question {id}")
                        on:click=toggle_edit
                    >
                        "Edit"
                    </button>
                </div>
            </div>
            {move || {
                faults
                    .with(|faults| {
                        faults
                            .iter()
                            .map(|msg| {
                                view! {
                                    <p class="channel-fault" data-testid="channel-fault">
                                        <span class="fault-lamp" aria-hidden="true"></span>
                                        {msg.clone()}
                                    </p>
                                }
                            })
                            .collect_view()
                    })
            }}
            <Show when=move || open.get()>
                <Program deck=deck id=key dom=dom.clone() spec=spec />
            </Show>
            {move || {
                let (answer, _) = shown.get();
                spec.with(|spec| answer_view(&answer_id, answer, spec))
            }}
        </li>
    }
}

/// The channel's readout: the answer's ladders and figures, raw JSON for an answer of an unknown type, or the
/// question's options on dark ladders before a run.
fn answer_view(id: &str, answer: Option<Shown>, spec: &Value) -> AnyView {
    match answer {
        Some(Shown::Typed(typed)) => match *typed {
            Answer::Choice(a) => choice(id, a).into_any(),
            Answer::Score(a) => score(id, a).into_any(),
            Answer::Noul(a) => noul(id, a).into_any(),
        },
        Some(Shown::Raw(raw)) => view! {
            <div class="raw-answer">
                <p class="legend">"Answer of an unknown type, as returned"</p>
                <pre data-testid="raw-answer">
                    {serde_json::to_string_pretty(&raw).unwrap_or_default()}
                </pre>
            </div>
        }
        .into_any(),
        None => idle(spec["type"].as_str().unwrap_or("unknown"), spec).into_any(),
    }
}

/// One option's ladder: name, the LED ladder at `level` (0..1), the exact readout, lit when it is the answer.
fn ladder(
    name: String,
    note: Option<String>,
    level: f64,
    readout: AnyView,
    lit: bool,
) -> impl IntoView {
    view! {
        <li class="ladder-row" class:lit=lit data-testid="ladder">
            <span class="lamp" aria-hidden="true"></span>
            <span class="ladder-name">
                {name}
                {lit.then(|| view! { <span class="visually-hidden">" (answer)"</span> })}
                {note.map(|note| view! { <span class="ladder-note">{note}</span> })}
            </span>
            <span class="ladder" aria-hidden="true" style=("--level", level.clamp(0.0, 1.0).to_string())>
                <span class="ladder-fill"></span>
            </span>
            {readout}
        </li>
    }
}

fn number(field: String, value: f64, format: Format) -> AnyView {
    view! { <Number field=field value=value format=format class="readout" /> }.into_any()
}

/// A figure under the ladders: engraved legend, readout.
fn figure(legend: &'static str, value: AnyView) -> impl IntoView {
    view! {
        <div class="figure">
            <dt class="legend">{legend}</dt>
            <dd>{value}</dd>
        </div>
    }
}

fn choice(id: &str, a: ChoiceAnswer) -> impl IntoView {
    let field = |parts: &[&str]| pointer(["answers", id].into_iter().chain(parts.iter().copied()));
    let rows = a
        .probabilities
        .iter()
        .map(|(name, p)| {
            let readout = number(field(&["probabilities", name]), *p, Format::Percent);
            ladder(name.clone(), None, *p, readout, *name == a.choice)
        })
        .collect_view();
    view! {
        <ol class="ladders">{rows}</ol>
        <dl class="figures">
            {figure(
                "Answer",
                view! { <Verbatim field=field(&["choice"]) value=a.choice.clone() class="figure-text" /> }
                    .into_any(),
            )}
            {figure("Confidence", number(field(&["confidence"]), a.confidence, Format::Fixed2))}
            {a.x_p_max.map(|v| figure("P max", number(field(&["x_p_max"]), v, Format::Percent)))}
            {a.x_certainty.map(|v| figure("Certainty", number(field(&["x_certainty"]), v, Format::Fixed2)))}
        </dl>
    }
}

fn score(id: &str, a: ScoreAnswer) -> impl IntoView {
    let field = |parts: &[&str]| pointer(["answers", id].into_iter().chain(parts.iter().copied()));
    let top = a.probabilities.values().copied().fold(f64::MIN, f64::max);
    let rows = a
        .probabilities
        .iter()
        .map(|(level, p)| {
            let fit = a.x_level_fit.as_ref().and_then(|fits| fits.get(level)).map(|fit| {
                view! {
                    <span class="ladder-fit">
                        "fit "
                        <Number field=field(&["x_level_fit", level]) value=*fit format=Format::Percent class="fit" />
                    </span>
                }
            });
            let readout = view! {
                {number(field(&["probabilities", level]), *p, Format::Percent)}
                {fit}
            }
            .into_any();
            ladder(level.clone(), a.legend.get(level).cloned(), *p, readout, *p == top)
        })
        .collect_view();
    view! {
        <ol class="ladders">{rows}</ol>
        <dl class="figures">
            {figure("Score", number(field(&["score"]), a.score, Format::Fixed2))}
            {figure("Confidence", number(field(&["confidence"]), a.confidence, Format::Fixed2))}
            {a.x_p_max.map(|v| figure("P max", number(field(&["x_p_max"]), v, Format::Percent)))}
            {a.x_certainty.map(|v| figure("Certainty", number(field(&["x_certainty"]), v, Format::Fixed2)))}
            {a.x_fit_mass.map(|v| figure("Fit mass", number(field(&["x_fit_mass"]), v, Format::Fixed2)))}
        </dl>
    }
}

fn noul(id: &str, a: NoulAnswer) -> impl IntoView {
    let readout = number(pointer(["answers", id, "noul"]), a.noul, Format::Fixed2);
    view! {
        <ol class="ladders">
            {ladder("noul".into(), Some("probability of yes".into()), a.noul, readout, false)}
        </ol>
    }
}

/// Before a run: the options the question names, on dark ladders.
fn idle(kind: &str, spec: &Value) -> impl IntoView {
    let criteria = &spec["criteria"];
    let options: Vec<(String, Option<String>)> = match (kind, criteria) {
        ("noul", _) => vec![("noul".into(), Some("probability of yes".into()))],
        ("score", Value::Array(levels)) => levels
            .iter()
            .enumerate()
            .map(|(i, level)| (i.to_string(), Some(text(level))))
            .collect(),
        ("choice", Value::Array(names)) => names.iter().map(|name| (text(name), None)).collect(),
        (_, Value::Object(map)) => map
            .iter()
            .map(|(name, note)| (name.clone(), (!note.is_null()).then(|| text(note))))
            .collect(),
        _ => Vec::new(),
    };
    view! {
        <ol class="ladders">
            {options
                .into_iter()
                .map(|(name, note)| {
                    let readout = view! {
                        <span class="readout idle">
                            <span aria-hidden="true">"\u{a0}"</span>
                            <span class="visually-hidden">"not run yet"</span>
                        </span>
                    }
                    .into_any();
                    ladder(name, note, 0.0, readout, false)
                })
                .collect_view()}
        </ol>
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

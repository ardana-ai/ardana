//! The questions column: the last run's line, the fault callout, one block per question with its type control, its
//! builder (opened by Edit) and a bar per option grown to the probability the API returned, the "Add question" row,
//! and the `questions` JSON the blocks are read from.

use ardana_api::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer};
use leptos::prelude::*;
use serde_json::Value;

use super::builder::Program;
use super::controls::Toggle;
use super::dom_id;
use super::figure::{Format, Number, Verbatim, pointer};
use super::icons::Icon;
use crate::builder::{self, KINDS, kind_name};
use crate::deck::Deck;
use crate::request::{Detail, Reply, Shown};

const EXAMPLE: &str = r#"{
  "refund": {
    "type": "noul",
    "instructions": "Does the customer ask for a refund?"
  }
}"#;

#[component]
pub fn Questions(deck: Deck) -> impl IntoView {
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
        if let Some(id) = &added {
            super::focus_later(format!("{}-id", dom_id(id)));
        }
        deck.editing.set(added);
    };
    view! {
        <section class="questions" aria-labelledby="questions-title">
            <div class="block-head">
                <h2 id="questions-title" class="block-heading">"Questions"</h2>
                <LastRun deck=deck />
            </div>
            <Fault deck=deck />
            <Show
                when=move || deck.questions.with(|q| !q.is_empty())
                fallback=|| {
                    view! {
                        <div class="callout">
                            <div class="callout-row">
                                <Icon name="info" />
                                <p>
                                    "No questions yet. Add one below and set its type: " <code>"noul"</code>
                                    " for a yes/no probability, " <code>"choice"</code> " for named options, "
                                    <code>"score"</code>
                                    " for ordered levels. Or load a preset from the sidebar, paste questions JSON below, or open a share link."
                                </p>
                            </div>
                        </div>
                    }
                }
            >
                <ol class="question-list">
                    <For each=ids key=|id| id.clone() children=move |id| view! { <Question deck=deck id=id /> } />
                </ol>
            </Show>
            <button type="button" id="add-question" class="row add-row" on:click=add>
                <Icon name="plus" />
                "Add question"
            </button>
            <div class="program">
                <label class="field-label" for="questions">"Questions JSON"</label>
                <textarea
                    id="questions"
                    class="code-block program-text"
                    spellcheck="false"
                    placeholder=EXAMPLE
                    aria-invalid=move || deck.questions_error.with(Option::is_some).to_string()
                    aria-describedby="questions-error"
                    prop:value=move || deck.questions_text.get()
                    on:input=move |event| deck.set_questions_text(event_target_value(&event))
                ></textarea>
                <p id="questions-error" class="fault-line">
                    {move || deck.questions_error.get()}
                </p>
            </div>
        </section>
    }
}

/// The last run in one line: who answered, how long it took, what it cost in tokens, and whether the inputs have
/// moved on since.
#[component]
fn LastRun(deck: Deck) -> impl IntoView {
    move || {
        deck.last.with(|last| {
            let run = last.as_ref()?;
            let ms = run.exchange.latency_ms;
            let latency = view! {
                <span class="dot" aria-hidden="true">"·"</span>
                <span data-testid="latency">
                    <span data-ms=ms.to_string()>{format!("{ms:.0}")}</span>
                    " ms"
                </span>
            };
            let stale = move || {
                deck.stale.get().then(|| {
                    view! {
                        <span class="tag" data-testid="stale">"From last run · inputs changed"</span>
                    }
                })
            };
            Some(match &run.reply {
                Ok(Reply::Answered { model, .. }) => view! {
                    <p class="last-run" data-testid="answered-by">
                        "Answered by "
                        <Verbatim field="/model".to_string() value=model.clone() class="" />
                        {latency}
                        <span class="dot" aria-hidden="true">"·"</span>
                        <span data-testid="tokens-in">
                            {usage(deck, |i, _| i, "input_tokens")}
                            " input"
                        </span>
                        <span class="dot" aria-hidden="true">"·"</span>
                        <span data-testid="tokens-out">
                            {usage(deck, |_, o| o, "output_tokens")}
                            " output tokens"
                        </span>
                        {stale}
                    </p>
                }
                .into_any(),
                Ok(Reply::Failed { status, .. }) => view! {
                    <p class="last-run">"Not answered: HTTP " {*status} {latency} {stale}</p>
                }
                .into_any(),
                Err(_) => view! { <p class="last-run">"No response" {latency} {stale}</p> }.into_any(),
            })
        })
    }
}

/// One of the last run's token counts, from `usage`, as a figure.
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
                view! { <Verbatim field=format!("/usage/{key}") value=n.to_string() class="" /> }.into_any()
            }),
            _ => None,
        })
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
                        Detail::Other(text) => {
                            view! { <pre class="code-block fault-raw">{text.clone()}</pre> }.into_any()
                        }
                    };
                    (format!("HTTP {status}"), body)
                }
            };
            Some(view! {
                <div class="callout callout-danger" data-testid="fault">
                    <div class="callout-row">
                        <Icon name="alert" />
                        <p class="fault-title">{title} " · the request was not answered."</p>
                    </div>
                    {body}
                </div>
            })
        })
    }
}

#[component]
fn Question(deck: Deck, id: String) -> impl IntoView {
    let key = StoredValue::new(id.clone());
    let spec = Memo::new(move |_| {
        key.with_value(|id| {
            deck.questions
                .with(|q| q.get(id).cloned().unwrap_or(Value::Null))
        })
    });
    // This question's answer in the last run, and the run's number, so each run's bars are drawn afresh.
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
    // Whether this question's answer came from a request whose state, model or question differs from now.
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
            class="question"
            class:editing=open
            class:stale=move || stale.get()
            class:faulted=move || faults.with(|f| !f.is_empty())
            data-testid="channel"
            data-question=id.clone()
        >
            <div class="question-head">
                <h3 class="question-id">{id.clone()}</h3>
                <div class="question-controls">
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
                        class="button button-sm"
                        aria-expanded=move || open.get().to_string()
                        aria-controls=format!("{dom}-program")
                        aria-label=format!("Edit question {id}")
                        on:click=toggle_edit
                    >
                        "Edit"
                    </button>
                </div>
            </div>
            {move || instructions().map(|text| view! { <p class="question-instructions">{text}</p> })}
            {move || {
                stale
                    .get()
                    .then(|| {
                        view! { <span class="tag" data-testid="stale">"From last run · inputs changed"</span> }
                    })
            }}
            {move || {
                faults
                    .with(|faults| {
                        faults
                            .iter()
                            .map(|msg| {
                                view! {
                                    <p class="question-fault" data-testid="channel-fault">
                                        <Icon name="alert" />
                                        <span>{msg.clone()}</span>
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

/// The question's answer: its bars and properties, raw JSON for an answer of an unknown type, or the question's
/// options on empty bars before a run.
fn answer_view(id: &str, answer: Option<Shown>, spec: &Value) -> AnyView {
    match answer {
        Some(Shown::Typed(typed)) => match *typed {
            Answer::Choice(a) => choice(id, a).into_any(),
            Answer::Score(a) => score(id, a).into_any(),
            Answer::Noul(a) => noul(id, a).into_any(),
        },
        Some(Shown::Raw(raw)) => view! {
            <div class="raw-answer">
                <p>"Answer of an unknown type, as returned"</p>
                <pre class="code-block" data-testid="raw-answer">
                    {serde_json::to_string_pretty(&raw).unwrap_or_default()}
                </pre>
            </div>
        }
        .into_any(),
        None => idle(spec["type"].as_str().unwrap_or("unknown"), spec).into_any(),
    }
}

/// One option's row: the check mark, the name, the bar at `level` (0..1), the exact readout; the answer is checked.
/// A noul row is `unmarked`: it is a probability, not a pick, so it carries no box.
fn bar(
    name: String,
    note: Option<String>,
    level: f64,
    readout: AnyView,
    lit: bool,
    marked: bool,
) -> impl IntoView {
    view! {
        <li class="bar-row" class:lit=lit class:unmarked=!marked data-testid="ladder">
            <span class="bar-mark" aria-hidden="true">
                <Icon name="check" />
            </span>
            <span class="bar-name">
                {name}
                {lit.then(|| view! { <span class="visually-hidden">" (answer)"</span> })}
                {note.map(|note| view! { <span class="bar-note">{note}</span> })}
            </span>
            <span class="bar" aria-hidden="true" style=("--level", level.clamp(0.0, 1.0).to_string())>
                <span class="bar-fill"></span>
            </span>
            {readout}
        </li>
    }
}

fn number(field: String, value: f64, format: Format) -> AnyView {
    view! { <Number field=field value=value format=format class="readout" /> }.into_any()
}

/// A property under the bars: its glyph, its name with a one-line meaning in a tooltip, its value.
fn property(
    icon: &'static str,
    name: &'static str,
    meaning: &'static str,
    value: AnyView,
) -> impl IntoView {
    view! {
        <div class="property">
            <dt>
                <Icon name=icon />
                <dfn class="dfn tip tip-start" data-tip=meaning tabindex="0">{name}</dfn>
                <span class="visually-hidden">": " {meaning}</span>
            </dt>
            <dd>{value}</dd>
        </div>
    }
}

const CONFIDENCE: &str =
    "How far the top probability stands above an even split: (n × p max − 1) / (n − 1).";
const P_MAX: &str = "The largest probability among the options.";
const CERTAINTY: &str = "One minus the normalised entropy of the probabilities.";

fn choice(id: &str, a: ChoiceAnswer) -> impl IntoView {
    let field = |parts: &[&str]| pointer(["answers", id].into_iter().chain(parts.iter().copied()));
    let rows = a
        .probabilities
        .iter()
        .map(|(name, p)| {
            let readout = number(field(&["probabilities", name]), *p, Format::Percent);
            bar(name.clone(), None, *p, readout, *name == a.choice, true)
        })
        .collect_view();
    view! {
        <ol class="bars">{rows}</ol>
        <dl class="properties">
            {property(
                "check",
                "Answer",
                "The most probable option.",
                view! { <Verbatim field=field(&["choice"]) value=a.choice.clone() class="answer" /> }.into_any(),
            )}
            {property("target", "Confidence", CONFIDENCE, number(field(&["confidence"]), a.confidence, Format::Fixed2))}
            {a.x_p_max.map(|v| property("hash", "P max", P_MAX, number(field(&["x_p_max"]), v, Format::Percent)))}
            {a.x_certainty
                .map(|v| property("gauge", "Certainty", CERTAINTY, number(field(&["x_certainty"]), v, Format::Fixed2)))}
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
                    <span class="bar-fit">
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
            bar(level.clone(), a.legend.get(level).cloned(), *p, readout, *p == top, true)
        })
        .collect_view();
    view! {
        <ol class="bars">{rows}</ol>
        <dl class="properties">
            {property(
                "gauge",
                "Score",
                "The expected level: each level weighted by its probability, to two places.",
                number(field(&["score"]), a.score, Format::Fixed2),
            )}
            {property("target", "Confidence", "The score's confidence, as the API returned it.", number(field(&["confidence"]), a.confidence, Format::Fixed2))}
            {a.x_p_max.map(|v| property("hash", "P max", P_MAX, number(field(&["x_p_max"]), v, Format::Percent)))}
            {a.x_certainty
                .map(|v| property("gauge", "Certainty", CERTAINTY, number(field(&["x_certainty"]), v, Format::Fixed2)))}
            {a.x_fit_mass
                .map(|v| {
                    property(
                        "hash",
                        "Fit mass",
                        "The sum of the level fits before normalising (isolated levels only).",
                        number(field(&["x_fit_mass"]), v, Format::Fixed2),
                    )
                })}
        </dl>
    }
}

fn noul(id: &str, a: NoulAnswer) -> impl IntoView {
    let readout = number(pointer(["answers", id, "noul"]), a.noul, Format::Fixed2);
    view! {
        <ol class="bars">
            {bar("noul".into(), Some("probability of yes".into()), a.noul, readout, false, false)}
        </ol>
    }
}

/// Before a run: the options the question names, on empty bars.
fn idle(kind: &str, spec: &Value) -> impl IntoView {
    let criteria = &spec["criteria"];
    let marked = kind != "noul";
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
        <ol class="bars">
            {options
                .into_iter()
                .map(|(name, note)| {
                    let readout = view! {
                        <span class="readout idle">
                            <span aria-hidden="true">"–"</span>
                            <span class="visually-hidden">"not run yet"</span>
                        </span>
                    }
                    .into_any();
                    bar(name, note, 0.0, readout, false, marked)
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

//! The channels: one row per question, with its engraved id, type, and an amber ladder per option that climbs
//! to the probability the API returned. Below them, the `questions` JSON the channels are read from.

use ardana_api::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer};
use leptos::prelude::*;
use serde_json::Value;

use super::figure::{Format, Number, Verbatim, pointer};
use super::run_shortcut;
use crate::deck::Deck;
use crate::request::{Detail, Reply, Shown};

const EXAMPLE: &str = r#"Example:
{
  "refund": {
    "type": "noul",
    "instructions": "Does the customer ask for a refund?"
  }
}"#;

/// What one channel shows: its question, and its answer from the last run when there is one.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    id: String,
    spec: Value,
    answer: Option<Shown>,
    run: usize,
}

#[component]
pub fn Channels(deck: Deck) -> impl IntoView {
    let items = move || {
        let (answers, run) = deck.last.with(|last| match last {
            Some(run) => match &run.reply {
                Ok(Reply::Answered { answers, .. }) => (Some(answers.clone()), run.number),
                _ => (None, run.number),
            },
            None => (None, 0),
        });
        deck.questions.with(|questions| {
            questions
                .iter()
                .map(|(id, spec)| Item {
                    id: id.clone(),
                    spec: spec.clone(),
                    answer: answers.as_ref().and_then(|a| a.get(id).cloned()),
                    run,
                })
                .collect::<Vec<_>>()
        })
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
                                "No channels yet. Write a questions map below, one entry per question: "
                                <code>"noul"</code> " for a yes/no probability, " <code>"choice"</code>
                                " for named options, " <code>"score"</code>
                                " for ordered levels. Or open a Jev share link."
                            </p>
                        </div>
                    }
                }
            >
                <ol class="channel-list">
                    <For
                        each=items
                        key=|item| (item.id.clone(), item.spec.to_string(), item.run, item.answer.is_some())
                        children=|item| view! { <Channel item=item /> }
                    />
                </ol>
            </Show>
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
                <p id="questions-error" class="fault-line" role="alert">
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
                <div class="fault" role="alert" data-testid="fault">
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
fn Channel(item: Item) -> impl IntoView {
    let Item {
        id, spec, answer, ..
    } = item;
    let kind = match &answer {
        Some(Shown::Typed(typed)) => match **typed {
            Answer::Choice(_) => "choice".to_string(),
            Answer::Score(_) => "score".to_string(),
            Answer::Noul(_) => "noul".to_string(),
        },
        Some(Shown::Raw(raw)) => raw["type"].as_str().unwrap_or("unknown").to_string(),
        None => spec["type"].as_str().unwrap_or("unknown").to_string(),
    };
    let instructions = match &spec["instructions"] {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    };
    let body = match answer {
        Some(Shown::Typed(typed)) => match *typed {
            Answer::Choice(a) => choice(&id, a).into_any(),
            Answer::Score(a) => score(&id, a).into_any(),
            Answer::Noul(a) => noul(&id, a).into_any(),
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
        None => idle(&kind, &spec).into_any(),
    };
    view! {
        <li class="channel" data-testid="channel" data-question=id.clone()>
            <div class="channel-head">
                <div class="channel-title">
                    <h3 class="channel-id">{id.clone()}</h3>
                    {instructions.map(|text| view! { <p class="channel-instructions">{text}</p> })}
                </div>
                <TypeIndicator kind=kind />
            </div>
            {body}
        </li>
    }
}

/// The question's type, engraved beside its id; the questions JSON is where it is changed.
#[component]
fn TypeIndicator(kind: String) -> impl IntoView {
    view! {
        <p class="type-legend" data-testid="type">
            <span class="type-key">"Type"</span>
            " "
            <span class="type-name">{kind}</span>
        </p>
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

//! A question's builder: its id, instructions and criteria as fields. Every edit rewrites the questions JSON at once;
//! JSON structure the fields cannot hold is shown read only and left to the JSON editor.

use leptos::prelude::*;
use serde_json::Value;

use super::focus_later;
use super::icons::Icon;
use crate::builder::{self, CHOICE_OPTIONS, Criteria, Field, SCORE_LEVELS};
use crate::deck::Deck;

/// Shows a field's error beside it and announces it politely only when the field turns invalid or valid again,
/// not on every keystroke.
fn report(deck: Deck, error: RwSignal<Option<String>>, field: &str, result: Result<(), String>) {
    let was_invalid = error.with_untracked(Option::is_some);
    match result {
        Ok(()) => {
            if was_invalid {
                deck.notice.set(format!("{field} is valid again"));
            }
            error.set(None);
        }
        Err(err) => {
            if !was_invalid {
                deck.notice.set(format!("{field}: {err}"));
            }
            error.set(Some(err));
        }
    }
}

/// After a row is removed, focus the Remove button of the row that took its place, else the Add button.
fn focus_after_removal(dom: &str, kind: &str, index: usize, count: usize, least: usize) {
    focus_later(if index < count && count > least {
        format!("{dom}-{kind}-{index}-remove")
    } else {
        format!("{dom}-add-{kind}")
    });
}

/// The criteria editor a question gets.
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Noul,
    Choice,
    Score,
    Unreadable(String),
}

#[component]
pub fn Program(
    deck: Deck,
    id: StoredValue<String>,
    dom: String,
    spec: Memo<Value>,
) -> impl IntoView {
    let edit = move |change: &dyn Fn(&mut Value) -> Result<(), String>| {
        id.with_value(|id| deck.edit_question(id, change))
    };
    let criteria = Memo::new(move |_| spec.with(builder::criteria));
    let shape = Memo::new(move |_| {
        criteria.with(|c| match c {
            Criteria::Noul { .. } => Shape::Noul,
            Criteria::Choice(_) => Shape::Choice,
            Criteria::Score(_) => Shape::Score,
            Criteria::Unreadable(why) => Shape::Unreadable(why.clone()),
        })
    });

    let id_error = RwSignal::new(None::<String>);
    // Whether the rename is being committed by Tab: `change` fires before focus has moved, so the key itself is
    // the signal. The block is drawn again under its new id, and focus follows the Tab to Instructions there.
    let tabbed = StoredValue::new(false);
    let rename = move |event| {
        let new = event_target_value(&event);
        let old = id.get_value();
        let renamed = deck.rename_question(&old, &new);
        let moved = renamed.is_ok() && old != new;
        report(deck, id_error, "Question id", renamed);
        if moved {
            let field = if tabbed.get_value() {
                "instructions"
            } else {
                "id"
            };
            focus_later(format!("{}-{field}", super::dom_id(&new)));
        }
        tabbed.set_value(false);
    };
    let remove = move |_| {
        let next = id.with_value(|id| deck.remove_question(id));
        focus_later(next.map_or_else(
            || "add-question".to_string(),
            |next| format!("{}-edit", super::dom_id(&next)),
        ));
    };
    let instructions = Memo::new(move |_| spec.with(builder::instructions));
    let panel_id = format!("{dom}-program");
    let id_input = format!("{dom}-id");
    let id_error_line = format!("{dom}-id-error");
    let instructions_input = format!("{dom}-instructions");

    view! {
        <div class="program-panel" id=panel_id>
            <div class="field">
                <label class="field-label" for=id_input.clone()>"Question id"</label>
                <input
                    id=id_input
                    class="field-input field-code"
                    type="text"
                    spellcheck="false"
                    prop:value=move || id.get_value()
                    aria-invalid=move || id_error.with(Option::is_some).to_string()
                    aria-describedby=id_error_line.clone()
                    on:keydown=move |event| tabbed.set_value(event.key() == "Tab" && !event.shift_key())
                    on:focus=move |_| tabbed.set_value(false)
                    on:change=rename
                />
                <p id=id_error_line class="fault-line field-error">{move || id_error.get()}</p>
            </div>
            <div class="field">
                <label class="field-label" for=instructions_input.clone()>"Instructions"</label>
                <textarea
                    id=instructions_input
                    class="field-input"
                    rows="2"
                    prop:value=move || instructions.with(|f| f.text().to_string())
                    disabled=move || !instructions.with(Field::editable)
                    on:input=move |event| {
                        let text = event_target_value(&event);
                        let _ = edit(&|spec| {
                            builder::set_instructions(spec, &text);
                            Ok(())
                        });
                    }
                ></textarea>
                {move || (!instructions.with(Field::editable)).then(structured_note)}
            </div>
            {move || match shape.get() {
                Shape::Noul => noul(dom.clone(), criteria, edit).into_any(),
                Shape::Choice => choice(deck, dom.clone(), criteria, edit).into_any(),
                Shape::Score => score(dom.clone(), criteria, edit).into_any(),
                Shape::Unreadable(why) => view! {
                    <p class="field-note">
                        {format!("These criteria stay in the questions JSON: {why}.")}
                    </p>
                }
                .into_any(),
            }}
            <div class="program-foot">
                <button type="button" class="button button-sm" on:click=remove>
                    <Icon name="trash" />
                    "Remove question"
                </button>
            </div>
        </div>
    }
}

fn structured_note() -> impl IntoView {
    view! { <p class="field-note">"Structured JSON: edit it in the questions JSON below."</p> }
}

/// An icon button that removes a row; held with `aria-disabled` at the lower bound, so it keeps focus.
fn remove_button(
    id: String,
    label: String,
    held: impl Fn() -> bool + Copy + Send + Sync + 'static,
    act: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <button
            type="button"
            id=id
            class="button button-icon button-sm tip tip-end"
            aria-label=label.clone()
            data-tip=label
            aria-disabled=move || held().to_string()
            on:click=move |_| {
                if !held() {
                    act();
                }
            }
        >
            <Icon name="trash" />
        </button>
    }
}

/// The row that adds one more; held with `aria-disabled` at the upper bound.
fn add_button(
    id: String,
    label: &'static str,
    held: impl Fn() -> bool + Copy + Send + Sync + 'static,
    act: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <div class="criteria-foot">
            <button
                type="button"
                id=id
                class="button button-sm"
                aria-disabled=move || held().to_string()
                on:click=move |_| {
                    if !held() {
                        act();
                    }
                }
            >
                <Icon name="plus" />
                {label}
            </button>
        </div>
    }
}

/// What a true and a false answer mean, both optional.
fn noul(
    dom: String,
    criteria: Memo<Criteria>,
    edit: impl Fn(&dyn Fn(&mut Value) -> Result<(), String>) -> Result<(), String>
    + Copy
    + Send
    + Sync
    + 'static,
) -> impl IntoView {
    let field = move |yes: bool| {
        criteria.with(|c| match c {
            Criteria::Noul { yes: y, no: n } => {
                if yes {
                    y.clone()
                } else {
                    n.clone()
                }
            }
            _ => Field::Text(String::new()),
        })
    };
    [
        (true, "Yes means (optional)"),
        (false, "No means (optional)"),
    ]
    .into_iter()
    .map(|(yes, label)| {
        let input = format!("{dom}-{}", if yes { "true" } else { "false" });
        view! {
            <div class="field">
                <label class="field-label" for=input.clone()>{label}</label>
                <input
                    id=input
                    class="field-input"
                    type="text"
                    prop:value=move || field(yes).text().to_string()
                    disabled=move || !field(yes).editable()
                    on:input=move |event| {
                        let text = event_target_value(&event);
                        let _ = edit(&|spec| builder::set_noul(spec, yes, &text));
                    }
                />
                {move || (!field(yes).editable()).then(structured_note)}
            </div>
        }
    })
    .collect_view()
}

/// The options: name, optional description, remove; 2..255 of them.
fn choice(
    deck: Deck,
    dom: String,
    criteria: Memo<Criteria>,
    edit: impl Fn(&dyn Fn(&mut Value) -> Result<(), String>) -> Result<(), String>
    + Copy
    + Send
    + Sync
    + 'static,
) -> impl IntoView {
    let dom = StoredValue::new(dom);
    let options = move || {
        criteria.with(|c| match c {
            Criteria::Choice(options) => options.clone(),
            _ => Vec::new(),
        })
    };
    let count = Memo::new(move |_| {
        criteria.with(|c| match c {
            Criteria::Choice(options) => options.len(),
            _ => 0,
        })
    });
    let row = move |index: usize| {
        let option = move || {
            options()
                .get(index)
                .cloned()
                .unwrap_or((String::new(), Field::Text(String::new())))
        };
        let error = RwSignal::new(None::<String>);
        let number = index + 1;
        let error_line = dom.with_value(|dom| format!("{dom}-option-{index}-error"));
        let remove = remove_button(
            dom.with_value(|dom| format!("{dom}-option-{index}-remove")),
            format!("Remove option {number}"),
            move || count.get() <= *CHOICE_OPTIONS.start(),
            move || {
                if edit(&|spec| builder::remove_option(spec, index)).is_ok() {
                    dom.with_value(|dom| {
                        focus_after_removal(
                            dom,
                            "option",
                            index,
                            count.get_untracked(),
                            *CHOICE_OPTIONS.start(),
                        )
                    });
                }
            },
        );
        view! {
            <li class="criteria-row">
                <input
                    class="field-input field-code"
                    type="text"
                    spellcheck="false"
                    aria-label=format!("Option {number} name")
                    aria-invalid=move || error.with(Option::is_some).to_string()
                    aria-describedby=error_line.clone()
                    prop:value=move || option().0
                    on:input=move |event| {
                        let name = event_target_value(&event);
                        let renamed = edit(&|spec| builder::set_option_name(spec, index, &name));
                        report(deck, error, &format!("Option {number} name"), renamed);
                    }
                />
                <input
                    class="field-input"
                    type="text"
                    aria-label=format!("Option {number} description")
                    placeholder="Description (optional)"
                    prop:value=move || option().1.text().to_string()
                    disabled=move || !option().1.editable()
                    on:input=move |event| {
                        let note = event_target_value(&event);
                        let _ = edit(&|spec| builder::set_option_note(spec, index, &note));
                    }
                />
                {remove}
                <p id=error_line class="fault-line criteria-error">{move || error.get()}</p>
            </li>
        }
    };
    view! {
        <div class="criteria">
            <p class="criteria-legend">
                {move || format!("Options · {} ({} to {})", count.get(), CHOICE_OPTIONS.start(), CHOICE_OPTIONS.end())}
            </p>
            <ol class="criteria-rows">
                <For each=move || 0..count.get() key=|index| *index children=row />
            </ol>
            {add_button(
                dom.with_value(|dom| format!("{dom}-add-option")),
                "Add option",
                move || count.get() >= *CHOICE_OPTIONS.end(),
                move || {
                    let _ = edit(&builder::add_option);
                },
            )}
        </div>
    }
}

/// The levels, lowest first: description, remove; 2..10 of them.
fn score(
    dom: String,
    criteria: Memo<Criteria>,
    edit: impl Fn(&dyn Fn(&mut Value) -> Result<(), String>) -> Result<(), String>
    + Copy
    + Send
    + Sync
    + 'static,
) -> impl IntoView {
    let dom = StoredValue::new(dom);
    let levels = move || {
        criteria.with(|c| match c {
            Criteria::Score(levels) => levels.clone(),
            _ => Vec::new(),
        })
    };
    let count = Memo::new(move |_| {
        criteria.with(|c| match c {
            Criteria::Score(levels) => levels.len(),
            _ => 0,
        })
    });
    let row = move |index: usize| {
        let level = move || {
            levels()
                .get(index)
                .cloned()
                .unwrap_or(Field::Text(String::new()))
        };
        let remove = remove_button(
            dom.with_value(|dom| format!("{dom}-level-{index}-remove")),
            format!("Remove level {index}"),
            move || count.get() <= *SCORE_LEVELS.start(),
            move || {
                if edit(&|spec| builder::remove_level(spec, index)).is_ok() {
                    dom.with_value(|dom| {
                        focus_after_removal(
                            dom,
                            "level",
                            index,
                            count.get_untracked(),
                            *SCORE_LEVELS.start(),
                        )
                    });
                }
            },
        );
        view! {
            <li class="criteria-row level-row">
                <span class="level-number" aria-hidden="true">{index}</span>
                <input
                    class="field-input"
                    type="text"
                    aria-label=format!("Level {index} description")
                    prop:value=move || level().text().to_string()
                    disabled=move || !level().editable()
                    on:input=move |event| {
                        let text = event_target_value(&event);
                        let _ = edit(&|spec| builder::set_level(spec, index, &text));
                    }
                />
                {remove}
            </li>
        }
    };
    view! {
        <div class="criteria">
            <p class="criteria-legend">
                {move || format!("Levels, lowest first · {} ({} to {})", count.get(), SCORE_LEVELS.start(), SCORE_LEVELS.end())}
            </p>
            <ol class="criteria-rows">
                <For each=move || 0..count.get() key=|index| *index children=row />
            </ol>
            {add_button(
                dom.with_value(|dom| format!("{dom}-add-level")),
                "Add level",
                move || count.get() >= *SCORE_LEVELS.end(),
                move || {
                    let _ = edit(&builder::add_level);
                },
            )}
        </div>
    }
}

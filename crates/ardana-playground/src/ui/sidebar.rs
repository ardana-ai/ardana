//! The sidebar: the logo, the model picker, the presets, the page's sections, and the origin the snippets aim at.
//! Below 960px it is a drawer the top bar opens.

use ardana_api::{ModelsResponse, human_size};
use leptos::ev;
use leptos::prelude::*;

use super::icons::Icon;
use super::logo::Logo;
use super::topbar::IN_MEMORY;
use super::{Shell, focus_later, reveal};
use crate::deck::{Deck, Runs, browser_value};
use crate::engine;
use crate::presets::PRESETS;

/// The page's sections, in order, as the "On this page" rows name them.
const SECTIONS: [&str; 3] = ["content", "exchange", "snippets"];

#[component]
pub fn Sidebar(
    deck: Deck,
    shell: Shell,
    models: LocalResource<Result<ModelsResponse, String>>,
) -> impl IntoView {
    let origin = window().location().host().unwrap_or_default();
    // The section in view: the last one whose top has passed under the top bar.
    let current = RwSignal::new(SECTIONS[0]);
    let locate = move || {
        let mut seen = SECTIONS[0];
        for id in SECTIONS {
            let Some(element) = document().get_element_by_id(id) else {
                continue;
            };
            if element.get_bounding_client_rect().top() <= 96.0 {
                seen = id;
            }
        }
        if current.get_untracked() != seen {
            current.set(seen);
        }
    };
    let scroll = window_event_listener(ev::scroll, move |_| locate());
    on_cleanup(move || scroll.remove());
    request_animation_frame(locate);
    // A jump opens a closed toggle block, scrolls the section into view and moves focus to it, so a pick in the
    // drawer never leaves focus on a hidden row. The drawer settles first: the page is inert while the drawer is open,
    // and focus into an inert subtree is a no-op.
    let jump = move |id: &'static str| {
        move |_| {
            shell.settle();
            reveal(id);
        }
    };
    view! {
        <nav class="sidebar" id="sidebar" aria-label="Workspace">
            <div class="workspace">
                <Logo />
                <button
                    type="button"
                    id="sidebar-close"
                    class="button button-icon button-sm tip tip-below tip-end"
                    aria-label="Close sidebar"
                    data-tip="Close sidebar\nCtrl+\\ or Cmd+\\"
                    on:click=move |_| shell.close()
                >
                    <Icon name="panel" />
                </button>
            </div>
            <div class="sidebar-section">
                <label class="sidebar-heading" for="model">"Model"</label>
                <ModelSelect deck=deck models=models />
            </div>
            <div class="sidebar-section" role="group" aria-labelledby="presets-heading">
                <p class="sidebar-heading" id="presets-heading">"Presets"</p>
                {PRESETS
                    .iter()
                    .map(|preset| {
                        view! {
                            <button
                                type="button"
                                class="row"
                                on:click=move |_| {
                                    let (state, questions) = preset.texts();
                                    deck.load_preset(state, questions);
                                    deck.notice.set(format!("Loaded the {} preset", preset.name));
                                    if shell.settle() {
                                        focus_later("state".to_string());
                                    }
                                }
                            >
                                <span class="row-label">{preset.name}</span>
                            </button>
                        }
                    })
                    .collect_view()}
                <Show when=move || deck.previous.with(Option::is_some)>
                    <button
                        type="button"
                        class="row row-undo"
                        on:click=move |_| {
                            deck.restore_previous();
                            shell.settle();
                            focus_later("state".to_string());
                        }
                    >
                        <Icon name="arrow" class="icon-back" />
                        <span class="row-label">"Restore previous"</span>
                    </button>
                </Show>
            </div>
            <div class="sidebar-section" role="group" aria-labelledby="sections-heading">
                <p class="sidebar-heading" id="sections-heading">"On this page"</p>
                {[("content", "State and questions"), ("exchange", "Raw exchange"), ("snippets", "Snippets")]
                    .into_iter()
                    .map(|(id, label)| {
                        view! {
                            <button
                                type="button"
                                class="row"
                                aria-current=move || (current.get() == id).then_some("true")
                                on:click=jump(id)
                            >
                                <span class="row-label">{label}</span>
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
            <p class="sidebar-foot">
                <span>"Serving " <code>{origin}</code></span>
                <span>"Ctrl+Enter or Cmd+Enter runs"</span>
            </p>
        </nav>
    }
}

/// The model picker: a real `select` naming the pulled models, the models whose browser variant runs in this tab,
/// then the library models this server has not pulled, which the ardana CLI runs (Q2). A model both pulled and
/// browser-capable has a row in each place (Q14).
#[component]
fn ModelSelect(deck: Deck, models: LocalResource<Result<ModelsResponse, String>>) -> impl IntoView {
    let failure = move || match models.get() {
        Some(Err(err)) => Some(view! { <p class="fault-line" role="alert">{err}</p> }),
        _ => None,
    };
    // A picked model the list does not name (a share link's) is offered as it is.
    let options = move || {
        let picked = deck.picked();
        let option = move |value: String, label: String| {
            let selected = picked == value;
            view! { <option value=value selected=selected>{label}</option> }
        };
        let sized = |name: &str, bytes: Option<u64>| match bytes {
            Some(bytes) => format!("{name} · {}", human_size(bytes)),
            None => name.to_string(),
        };
        deck.models.with(|models| {
            let pulled: Vec<_> = models
                .iter()
                .filter(|m| m.pulled())
                .map(|m| option(m.name.clone(), m.name.clone()))
                .collect();
            let browser: Vec<_> = models
                .iter()
                .filter(|m| m.x_browser.is_some())
                .map(|m| option(browser_value(&m.name), sized(&m.name, m.x_browser)))
                .collect();
            let library: Vec<_> = models
                .iter()
                .filter(|m| !m.pulled())
                .map(|m| option(m.name.clone(), sized(&m.name, m.x_size)))
                .collect();
            let name = deck.model.get_untracked();
            let unlisted = (!name.is_empty() && !models.iter().any(|m| m.name == name))
                .then(|| option(name.clone(), name.clone()));
            view! {
                {(!pulled.is_empty()).then(|| view! { <optgroup label="Pulled">{pulled}</optgroup> })}
                {(!browser.is_empty())
                    .then(|| view! { <optgroup label="In browser · runs in this tab">{browser}</optgroup> })}
                {(!library.is_empty())
                    .then(|| view! { <optgroup label="Library · runs with the ardana CLI">{library}</optgroup> })}
                {unlisted}
            }
        })
    };
    // A pick that does not run on this server says where Run answers it, and what its first run downloads: kept by
    // this browser, or, where the page cannot keep files, again on each visit; in the tab, what running it takes in
    // memory. A model this server has not pulled runs here once the ardana CLI pulls it; in the standalone build, which
    // no server serves, on the visitor's machine.
    let runs = Memo::new(move |_| deck.runs());
    // The options are drawn afresh whenever the list or the pick changes, their elements reused by place, and a select
    // keeps the selection its elements had (an option removed with its group, one drawn over another's place), not the
    // pick: a model pulled or removed meanwhile, listed again, moves to another group and left the select on its first
    // row. Once the options are drawn, the select shows the pick again.
    let select = NodeRef::<leptos::html::Select>::new();
    Effect::new(move |_| {
        deck.models.track();
        let picked = deck.picked();
        request_animation_frame(move || {
            if let Some(select) = select.get_untracked() {
                select.set_value(&picked);
            }
        });
    });
    let note = move || {
        let text = match runs.get() {
            Runs::Server => return None,
            Runs::Tab => tab_note(deck.browser_size()?, engine::keeps_files()),
            Runs::Cli => cli_note(deck.standalone, deck.pull_size()),
        };
        Some(view! {
            <p class="field-note model-note" id="model-note" data-testid="model-note">
                {text}
            </p>
        })
    };
    view! {
        <div class="select">
            <select
                id="model"
                class="select-input"
                node_ref=select
                aria-describedby=move || (runs.get() != Runs::Server).then_some("model-note")
                on:change=move |event| deck.pick(&event_target_value(&event))
            >
                {options}
            </select>
            <Icon name="chevron-down" />
        </div>
        {note}
        {failure}
    }
}

/// What the model note says of an "In browser" row: that it runs in this tab, what its first run downloads (`bytes`),
/// kept by this browser where the page can keep files (`keeps`), else again on each visit, and what running it then
/// takes in memory, which only a reload gives back whole.
fn tab_note(bytes: u64, keeps: bool) -> String {
    let size = human_size(bytes);
    let download = if keeps {
        format!("The first run downloads {size}, which this browser keeps.")
    } else {
        format!(
            "The first run on each visit downloads {size}: a page without HTTPS keeps no files."
        )
    };
    format!("Runs in this tab. {download} {IN_MEMORY}; only a reload frees all of it.")
}

/// What the model note says of a model no server runs here: on a server, that it is not pulled and how the ardana CLI
/// adds it, with what that downloads (`bytes`, when the list names the pick); in the `standalone` build, which no server
/// serves, that it runs with the ardana CLI on the visitor's machine.
fn cli_note(standalone: bool, bytes: Option<u64>) -> String {
    match (standalone, bytes) {
        (false, Some(bytes)) => format!(
            "Not pulled on this server. Pull it with the ardana CLI where the server runs ({}), then Run answers here.",
            human_size(bytes)
        ),
        (false, None) => {
            "Not pulled on this server. Pull it with the ardana CLI where the server runs, then Run answers here."
                .to_string()
        }
        (true, Some(bytes)) => format!(
            "Runs with the ardana CLI on your machine; its first run downloads {}.",
            human_size(bytes)
        ),
        (true, None) => "Runs with the ardana CLI on your machine.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_says_what_the_tab_downloads_and_holds() {
        assert_eq!(
            tab_note(467_748_928, true),
            "Runs in this tab. The first run downloads 467.7 MB, which this browser keeps. Running it takes 3 to 4 \
             times that in memory; only a reload frees all of it."
        );
        assert_eq!(
            tab_note(467_748_928, false),
            "Runs in this tab. The first run on each visit downloads 467.7 MB: a page without HTTPS keeps no files. \
             Running it takes 3 to 4 times that in memory; only a reload frees all of it."
        );
    }

    #[test]
    fn the_note_says_how_a_model_gets_here() {
        assert_eq!(
            cli_note(false, Some(2_708_804_640)),
            "Not pulled on this server. Pull it with the ardana CLI where the server runs (2.7 GB), then Run answers here."
        );
        assert_eq!(
            cli_note(true, Some(2_708_804_640)),
            "Runs with the ardana CLI on your machine; its first run downloads 2.7 GB."
        );
        assert_eq!(
            cli_note(true, None),
            "Runs with the ardana CLI on your machine."
        );
    }
}

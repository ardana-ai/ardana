//! The sidebar: the logo, the model picker, the presets, the page's sections, and the origin the snippets aim at.
//! Below 960px it is a drawer the top bar opens.

use ardana_api::{ModelsResponse, human_size};
use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::icons::Icon;
use super::logo::Logo;
use super::{Shell, focus_later, scroll_to};
use crate::deck::Deck;
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
    // A jump opens a closed toggle block, scrolls the section into view and moves focus to it (the block's summary,
    // or the page itself), so a pick in the drawer never leaves focus on a hidden row. The drawer settles first:
    // the page is inert while the drawer is open, and focus into an inert subtree is a no-op.
    let jump = move |id: &'static str| {
        move |_| {
            shell.settle();
            if let Some(element) = document().get_element_by_id(id) {
                if element.tag_name() == "DETAILS" {
                    let _ = element.set_attribute("open", "");
                }
                scroll_to(&element);
                // A toggle block hands focus to its own summary; the page takes it itself.
                let target = if element.tag_name() == "DETAILS" {
                    element
                        .query_selector(":scope > summary")
                        .ok()
                        .flatten()
                        .unwrap_or(element)
                } else {
                    element
                };
                request_animation_frame(move || {
                    if let Ok(target) = target.dyn_into::<web_sys::HtmlElement>() {
                        let options = web_sys::FocusOptions::new();
                        options.set_prevent_scroll(true);
                        let _ = target.focus_with_options(&options);
                    }
                });
            }
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
                    data-tip="Close sidebar\nCtrl+\\ or ⌘\\"
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
                <span>"Ctrl+Enter or ⌘↵ runs"</span>
            </p>
        </nav>
    }
}

/// The model picker: a real `select` naming the pulled models, then the library models a first run pulls.
#[component]
fn ModelSelect(deck: Deck, models: LocalResource<Result<ModelsResponse, String>>) -> impl IntoView {
    let failure = move || match models.get() {
        Some(Err(err)) => Some(view! { <p class="fault-line" role="alert">{err}</p> }),
        _ => None,
    };
    // A picked model the list does not name (a share link's) is offered as it is.
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
        <div class="select">
            <select
                id="model"
                class="select-input"
                prop:value=move || deck.model.get()
                on:change=move |event| deck.model.set(event_target_value(&event))
            >
                {options}
            </select>
            <Icon name="chevron-down" />
        </div>
        {failure}
    }
}

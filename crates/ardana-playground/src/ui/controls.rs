//! Shared controls: the segmented control (a native radio group, so arrow keys move the selection) and the copy
//! button with its toast.

use leptos::prelude::*;
use wasm_bindgen_futures::JsFuture;

use super::icons::Icon;

/// A segmented control: one segment per option, the checked one raised; a small legend names the group.
#[component]
pub fn Toggle<T>(
    legend: &'static str,
    /// The radio group's name, unique on the page.
    group: String,
    options: Vec<(T, &'static str)>,
    checked: impl Fn(T) -> bool + Copy + Send + Sync + 'static,
    pick: impl Fn(T) + Copy + Send + Sync + 'static,
    #[prop(optional)] testid: Option<&'static str>,
) -> impl IntoView
where
    T: Copy + Send + Sync + 'static,
{
    let legend_id = format!("{group}-legend");
    let labelled_by = legend_id.clone();
    view! {
        <div class="segmented" role="radiogroup" aria-labelledby=labelled_by data-testid=testid>
            <span class="segmented-legend" id=legend_id>{legend}</span>
            <span class="segmented-options">
                {options
                    .into_iter()
                    .map(|(value, label)| {
                        view! {
                            <label class="segment" class:on=move || checked(value)>
                                <input
                                    type="radio"
                                    class="segment-input"
                                    name=group.clone()
                                    value=label
                                    prop:checked=move || checked(value)
                                    on:change=move |_| pick(value)
                                />
                                {label}
                            </label>
                        }
                    })
                    .collect_view()}
            </span>
        </div>
    }
}

/// A button that copies `text()` to the clipboard and says so in a toast.
#[component]
pub fn CopyKey(
    /// What the button copies, for its accessible name: "Copy <what>".
    what: &'static str,
    text: impl Fn() -> String + Send + Sync + 'static,
) -> impl IntoView {
    let status = RwSignal::new(None::<String>);
    let copy = move |_| {
        let text = text();
        let Some(clipboard) = web_sys::window().map(|w| w.navigator().clipboard()) else {
            return;
        };
        leptos::task::spawn_local(async move {
            let done = JsFuture::from(clipboard.write_text(&text)).await;
            status.set(Some(match done {
                Ok(_) => format!("Copied the {what}"),
                Err(_) => {
                    "The browser refused the clipboard; select the text and copy it".to_string()
                }
            }));
            set_timeout(move || status.set(None), std::time::Duration::from_secs(4));
        });
    };
    view! {
        <span class="copy">
            <button type="button" class="button button-sm" aria-label=format!("Copy {what}") on:click=copy>
                <Icon name="copy" />
                "Copy"
            </button>
            <span class="toast" role="status">{move || status.get()}</span>
        </span>
    }
}

//! Shared controls: the segmented control (a native radio group, so arrow keys move the selection) and the copy
//! key with its status.

use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen_futures::JsFuture;

use super::icons::Icon;

/// A segmented control: one segment per option, the checked one outlined; a small legend names the group.
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

/// The copy key of ardana.ai's command boxes: copies `text()` to the clipboard, turns its glyph into a check for a
/// moment and announces it; a refusal is said in a toast.
#[component]
pub fn CopyKey(
    /// What the button copies, for its accessible name: "Copy <what>".
    what: &'static str,
    text: impl Fn() -> String + Send + Sync + 'static,
) -> impl IntoView {
    // The last attempt: whether it worked, and what the status region says about it.
    let status = RwSignal::new(None::<(bool, String)>);
    // Each attempt clears only its own status, so a second copy keeps its check for the whole moment.
    let attempts = StoredValue::new(0_u32);
    let copy = move |_| {
        let text = text();
        let Some(clipboard) = web_sys::window().map(|w| w.navigator().clipboard()) else {
            return;
        };
        leptos::task::spawn_local(async move {
            let worked = JsFuture::from(clipboard.write_text(&text)).await.is_ok();
            status.set(Some(if worked {
                (true, format!("Copied the {what}"))
            } else {
                (
                    false,
                    "The browser refused the clipboard; select the text and copy it".to_string(),
                )
            }));
            attempts.update_value(|n| *n += 1);
            let attempt = attempts.get_value();
            // The check stays as long as the landing's; a refusal stays long enough to read.
            let shown = Duration::from_millis(if worked { 1600 } else { 4000 });
            set_timeout(
                move || {
                    if attempts.get_value() == attempt {
                        status.set(None);
                    }
                },
                shown,
            );
        });
    };
    let copied = move || status.with(|s| matches!(s, Some((true, _))));
    view! {
        <span class="copy">
            <button
                type="button"
                class="copy-key tip tip-below tip-end"
                aria-label=format!("Copy {what}")
                data-tip=format!("Copy {what}")
                data-copied=move || copied().to_string()
                on:click=copy
            >
                <Icon name="copy" class="icon-copy" />
                <Icon name="check" class="icon-check" />
            </button>
            <span class="toast" class:quiet=copied role="status">
                {move || status.get().map(|(_, said)| said)}
            </span>
        </span>
    }
}

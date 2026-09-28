//! The fascia's small controls: the chrome toggle (a native radio group, so arrow keys move the selection) and the
//! copy key.

use leptos::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// A chrome toggle: one pressed key per option, its lamp lit; an engraved legend names the group.
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
        <div class="toggle" role="radiogroup" aria-labelledby=labelled_by data-testid=testid>
            <span class="legend toggle-legend" id=legend_id>{legend}</span>
            <span class="toggle-keys">
                {options
                    .into_iter()
                    .map(|(value, label)| {
                        view! {
                            <label class="toggle-key" class:on=move || checked(value)>
                                <input
                                    type="radio"
                                    class="toggle-input"
                                    name=group.clone()
                                    value=label
                                    prop:checked=move || checked(value)
                                    on:change=move |_| pick(value)
                                />
                                <span class="lamp" aria-hidden="true"></span>
                                <span class="toggle-name">{label}</span>
                            </label>
                        }
                    })
                    .collect_view()}
            </span>
        </div>
    }
}

/// A plate key that copies `text()` to the clipboard and says whether it did.
#[component]
pub fn CopyKey(
    /// What the key copies, for its accessible name: "Copy <what>".
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
                Ok(_) => "Copied".to_string(),
                Err(_) => {
                    "The browser refused the clipboard; select the text and copy it".to_string()
                }
            }));
            set_timeout(move || status.set(None), std::time::Duration::from_secs(4));
        });
    };
    view! {
        <span class="copy">
            <button type="button" class="plate-key" aria-label=format!("Copy {what}") on:click=copy>
                "Copy"
            </button>
            <span class="copy-status" role="status">{move || status.get()}</span>
        </span>
    }
}

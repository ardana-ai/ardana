//! The snippets: a toggle block with the request Run would send now, as curl, Python or TypeScript aimed at this
//! server, in one command box with its copy key; or, for a model this server does not run (one picked in the tab, or
//! one it has not pulled), the ardana CLI's commands (Q4): `ardana pull` first where a server lacks the model,
//! ardana.ai's install line first in the standalone build. The block builds its text only while it is open, and for a large
//! request once the editors pause, so typing in a long state never waits for it; a copy key copies the request as it
//! is at that moment.

use std::time::Duration;

use ardana_api::{SystemOneRequest, human_size};
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::controls::{CopyKey, Toggle};
use super::icons::Icon;
use crate::deck::{Deck, Handoff, Runs};
use crate::snippets::{self, INSTALL, INSTALL_WINDOWS, Language, snippet};

/// Editors holding more than this many bytes put the snippets off until they pause: Chrome lays a snippet out again
/// whenever its text changes, about 100 ms for a 400 KB state (on every key, before, a long task of 107–125 ms: finding
/// 6 of the design audit), while below this a key with Snippets open takes about 16 ms in all.
const LARGE: usize = 32 * 1024;
/// How long the editors of a large request stay unchanged before the snippets show it.
const SETTLE: Duration = Duration::from_millis(150);

#[component]
pub fn Snippets(deck: Deck) -> impl IntoView {
    let language = RwSignal::new(Language::Curl);
    let origin = window().location().origin().unwrap_or_default();
    // Whether the block is open: the toggle event follows a click on its summary and a jump that opens it alike.
    let open = RwSignal::new(true);
    let shown = shown_request(deck, open);
    let text = {
        let origin = origin.clone();
        Memo::new(move |_| {
            shown.with(|request| {
                request
                    .as_ref()
                    .map(|request| snippet(language.get(), &origin, request))
                    .unwrap_or_default()
            })
        })
    };
    let runs = Memo::new(move |_| deck.runs());
    view! {
        <details
            class="toggle"
            id="snippets"
            open=true
            on:toggle=move |event| {
                open.set(
                    event
                        .target()
                        .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                        .is_some_and(|details| details.has_attribute("open")),
                )
            }
        >
            <summary>
                <span class="toggle-marker">
                    <Icon name="chevron-right" />
                </span>
                <h2 class="toggle-heading">"Snippets"</h2>
                <span class="toggle-caption">"The request as it stands"</span>
            </summary>
            {move || match runs.get() {
                Runs::Server => {
                    let origin = origin.clone();
                    view! {
                        <div class="toggle-bar">
                            <Toggle
                                legend="Language"
                                group="snippet-language".to_string()
                                options=Language::ALL.map(|l| (l, l.name())).to_vec()
                                checked=move |l| language.get() == l
                                pick=move |l| language.set(l)
                            />
                        </div>
                        // The key comes first, as it did beside the language picker: Tab reaches it before the snippet.
                        <div class="snippet-box">
                            <CopyKey
                                what="snippet"
                                text=move || snippet(language.get_untracked(), &origin, &untrack(|| deck.request()))
                            />
                            <pre
                                class="code-block wire snippet"
                                tabindex="0"
                                role="group"
                                data-testid="snippet"
                                data-language=move || language.get().name()
                                aria-label=move || format!("{} snippet", language.get().name())
                            >
                                {move || text.get()}
                            </pre>
                        </div>
                    }
                        .into_any()
                }
                Runs::Tab | Runs::Cli => view! { <Cli deck=deck shown=shown /> }.into_any(),
            }}
        </details>
    }
}

/// The request the snippets show: nothing until the block is first open; while it is open, the request as it stands,
/// at once when the editors are small or the block has just opened, else once they have stayed unchanged for
/// [`SETTLE`]. A closed block builds nothing. The timer is the outside world, so an effect keeps the signal.
fn shown_request(deck: Deck, open: RwSignal<bool>) -> RwSignal<Option<SystemOneRequest>> {
    let shown = RwSignal::new(None);
    let mut settle = debounce(SETTLE, move |()| {
        if open.get_untracked() {
            shown.set(Some(untrack(|| deck.request())));
        }
    });
    Effect::new(move |was_open: Option<bool>| {
        let is_open = open.get();
        // Every change of the request (the pick, the state, the questions) runs this.
        let bytes = deck.state_text.with(String::len) + deck.questions_text.with(String::len);
        deck.model.track();
        deck.questions.track();
        if is_open {
            if bytes > LARGE && was_open == Some(true) {
                settle(());
            } else {
                shown.set(Some(untrack(|| deck.request())));
            }
        }
        is_open
    });
    shown
}

/// The request for the ardana CLI, where no server runs the pick ([`Handoff`]): in the standalone build, which no server
/// serves, ardana.ai's install line, then `ardana run` on the visitor's own machine; on a server, which ardana runs
/// already, `ardana pull` first when the server has not pulled the model (run where the server runs, after which
/// Run sends a server row there), then `ardana run`. Each command sits in a command box with its copy key; the model
/// note and the banner say why the page shows them.
#[component]
fn Cli(deck: Deck, shown: RwSignal<Option<SystemOneRequest>>) -> impl IntoView {
    let command = Memo::new(move |_| {
        shown.with(|request| request.as_ref().map(snippets::cli).unwrap_or_default())
    });
    let handoff = Memo::new(move |_| deck.handoff());
    // A server row (`Cli`) waits for the model on this server; an "In browser" row (`Tab`) runs here already.
    let server_row = Memo::new(move |_| deck.runs() == Runs::Cli);
    let size = move || deck.pull_size().map(human_size);
    let lede = move || {
        let model = deck.model.get();
        match (handoff.get(), server_row.get()) {
            (Handoff::Install, true) if !model.is_empty() => {
                format!("{model} runs with the ardana CLI on your machine.")
            }
            // An "In browser" row, or nothing picked yet (the standalone build before its list is in).
            (Handoff::Install, _) => {
                "The ardana CLI runs this request on your own machine.".to_string()
            }
            (Handoff::Pull, true) => {
                format!("This server runs {model} once the ardana CLI pulls it.")
            }
            (Handoff::Pull | Handoff::Run, _) => {
                "The ardana CLI runs this request in a terminal.".to_string()
            }
        }
    };
    let first = move || {
        match handoff.get() {
        Handoff::Install => Some(
            view! {
                <div class="cli-step">
                    <p class="field-label">"Install ardana"</p>
                    <div class="command-row">
                        <code data-testid="install">{INSTALL}</code>
                        <CopyKey what="install command" text=|| INSTALL.to_string() />
                    </div>
                    <p class="field-note">"On Windows, in PowerShell: " <code>{INSTALL_WINDOWS}</code></p>
                </div>
            }
                .into_any(),
        ),
        Handoff::Pull => {
            let (label, note) = if server_row.get() {
                let note = match size() {
                    Some(size) => format!("Downloads {size}; then Run answers here."),
                    None => "Then Run answers here.".to_string(),
                };
                ("Pull it where the server runs".to_string(), Some(note))
            } else {
                (
                    format!("Pull {} first", deck.model.get()),
                    size().map(|size| format!("Downloads {size}.")),
                )
            };
            Some(
                view! {
                    <div class="cli-step">
                        <p class="field-label">{label}</p>
                        <div class="command-row">
                            <code data-testid="pull">{move || snippets::pull(&deck.model.get())}</code>
                            <CopyKey
                                what="pull command"
                                text=move || snippets::pull(&deck.model.get_untracked())
                            />
                        </div>
                        {note.map(|note| view! { <p class="field-note" data-testid="pull-note">{note}</p> })}
                    </div>
                }
                    .into_any(),
            )
        }
        Handoff::Run => None,
    }
    };
    let run_label = move || match (handoff.get(), server_row.get()) {
        (Handoff::Pull, true) => "Or run the request in a terminal",
        (Handoff::Install | Handoff::Pull, _) => "Then run the request",
        (Handoff::Run, _) => "Run the request",
    };
    // What `ardana run` downloads first on the visitor's machine, when the list names the pick.
    let download = move || {
        if handoff.get() != Handoff::Install {
            return None;
        }
        let size = size()?;
        Some(view! {
            <p class="field-note">{format!("Its first run downloads {} ({size}).", deck.model.get())}</p>
        })
    };
    view! {
        <div class="cli">
            <p class="cli-lede" data-testid="cli-lede">{lede}</p>
            {first}
            <div class="cli-step">
                <p class="field-label">{run_label}</p>
                <div class="snippet-box">
                    <CopyKey what="command" text=move || snippets::cli(&untrack(|| deck.request())) />
                    <pre
                        class="code-block wire snippet"
                        tabindex="0"
                        role="group"
                        data-testid="snippet"
                        data-language="ardana"
                        aria-label="ardana command"
                    >
                        {move || command.get()}
                    </pre>
                </div>
                {download}
            </div>
        </div>
    }
}

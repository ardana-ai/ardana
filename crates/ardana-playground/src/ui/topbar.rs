//! The sticky top bar: the sidebar opener and the logo (while the sidebar is away), the page's name, the stale tag,
//! Share with its popover, and the ink Run pill. Under it, the banner that says where a run in this tab is (with Stop
//! while the server pulls or the tab downloads), why Run is held (with the way to the ardana CLI's commands when the
//! server does not run the model, and on a public server the browser default in this tab instead), or what a first
//! run in this tab downloads.

use ardana_api::human_size;
use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use super::controls::CopyKey;
use super::icons::Icon;
use super::logo::Logo;
use super::{Shell, focus_later, reveal};
use crate::deck::{Deck, Held, browser_value};
use crate::engine::{self, Stage};
use crate::share::{self, SharePayload};

/// What the banner says about a run of `name` (whose browser files hold `size` bytes) in this tab, and how far its
/// download is (bytes had, of the total); nothing while it waits a moment for the server's answer.
pub fn tab_note(
    name: &str,
    size: Option<u64>,
    stage: Stage,
) -> Option<(String, Option<(u64, u64)>)> {
    Some(match stage {
        Stage::Asking => return None,
        Stage::Pulling => (
            match size {
                Some(bytes) => format!(
                    "Pulling the browser files of {name} ({}) on the server",
                    human_size(bytes)
                ),
                None => format!("Pulling the browser files of {name} on the server"),
            },
            None,
        ),
        Stage::Downloading { done, total } => (
            format!(
                "Downloading {name} into this tab: {} of {}",
                human_size(done),
                human_size(total)
            ),
            Some((done, total)),
        ),
        Stage::Starting => (format!("Starting {name} in this tab"), None),
        Stage::Running(backend) => (
            format!("Running {name} in this tab on {}", backend.name()),
            None,
        ),
    })
}

/// What the run's status region says about a run in this tab: the banner's words, the download counted in tenths, so
/// a screen reader hears each step once.
pub fn tab_status(name: &str, size: Option<u64>, stage: Stage) -> Option<String> {
    match stage {
        Stage::Downloading { done, total } => Some(format!(
            "Downloading {name} into this tab: {}% of {}",
            done.saturating_mul(10).checked_div(total).unwrap_or(0) * 10,
            human_size(total)
        )),
        stage => tab_note(name, size, stage).map(|(text, _)| text),
    }
}

/// What a browser model takes in memory once run, said after what its first run downloads: 3 to 4 times its files.
/// Measured in Chrome 154 after a run of each browser model, as the page's memory after forced collections
/// (`performance.measureUserAgentSpecificMemory`) and the GPU process's footprint: decider-0.8b 3.4 times on WebGPU
/// (the page 585 MB, the GPU process 1.0 GB more) and 4.1 on WASM (1.94 GB in the page), qwen3.5-0.8b 3.3 and 2.7,
/// decider-2b 2.9 and 3.5 (`tmp/evals/u2/memory*.json`). Running again adds nothing; picking a server row frees part of
/// the GPU's share (625 MB of decider-0.8b's), and the page's WebAssembly memory never shrinks: only a reload frees all
/// of it.
pub const IN_MEMORY: &str = "Running it takes 3 to 4 times that in memory";

/// What the banner says before a run of `name` in this tab that downloads `size` bytes, where this page can keep them
/// (`keeps`) or cannot, and what running it then takes in memory.
fn first_download(name: &str, size: u64, keeps: bool) -> String {
    let size = human_size(size);
    if keeps {
        format!(
            "Run downloads {name} into this tab once: {size}, kept by this browser. {IN_MEMORY}."
        )
    } else {
        format!(
            "Run downloads {name} into this tab on each visit: {size}, as a page without HTTPS keeps no files. \
             {IN_MEMORY}."
        )
    }
}

/// The share of the viewport's height the bars may hold while the page scrolls under them.
const STUCK_SHARE: f64 = 1.0 / 3.0;

/// What of the top bar and the in-flight banner stays on screen while the page scrolls, given their heights (`bar`,
/// `banner`), the height of the banner's last row when its download bar and Stop take a row of their own (`row`), and
/// the viewport's (`viewport`): the bar, and all of the banner, or only its last row, or none of it; or nothing at all.
/// Together they hold at most [`STUCK_SHARE`] of the viewport, so a zoomed or enlarged page keeps most of its screen.
fn stuck(bar: f64, banner: f64, row: Option<f64>, viewport: f64) -> Stuck {
    let room = viewport * STUCK_SHARE;
    if bar > room {
        Stuck::Nothing
    } else if bar + banner <= room {
        Stuck::Banner
    } else if row.is_some_and(|row| bar + row <= room) {
        Stuck::Row
    } else {
        Stuck::Bar
    }
}

/// See [`stuck`]; the root element's classes say it to the stylesheets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stuck {
    /// The bar and the whole in-flight banner.
    Banner,
    /// The bar and the banner's last row (`banner-row`): its words scroll away under the bar.
    Row,
    /// The bar alone (`banner-loose`).
    Bar,
    /// Nothing (`topbar-loose` and `banner-loose`).
    Nothing,
}

/// The height of the banner's last row, from the end of its words to its own end, when its download bar (with Stop)
/// sits on a row of its own under the words.
fn banner_row(banner: &web_sys::Element) -> Option<f64> {
    let words = banner.query_selector("#run-note-text").ok()??;
    let bar = banner.query_selector(".banner-progress").ok()??;
    let end = words.get_bounding_client_rect().bottom();
    (bar.get_bounding_client_rect().top() >= end)
        .then(|| banner.get_bounding_client_rect().bottom() - end)
}

/// Keeps the root element's `--topbar-height`, `--banner-height` and `--banner-row-height` at the rendered heights of the
/// top bar (`header`), the banner and the banner's last row, and its `topbar-loose`, `banner-row` and `banner-loose`
/// classes at what of them stays on screen ([`stuck`]), whenever a box or the viewport changes size. `base.css` and
/// `shell.css` read them: the banner sticks under the bar while a run is in flight, all of it or its last row, and the
/// page scrolls a focused control clear of what stays (WCAG 2.4.11), whatever the width, the zoom and the text size.
fn follow_heights(header: &web_sys::Element, banner: &web_sys::Element) {
    let Some(root) = document()
        .document_element()
        .and_then(|root| root.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        return;
    };
    let (bar_element, banner_element) = (header.clone(), banner.clone());
    let settle = move || {
        let bar = bar_element.get_bounding_client_rect().height();
        let note = banner_element.get_bounding_client_rect().height();
        let row = banner_row(&banner_element);
        let style = root.style();
        for (property, height) in [
            ("--topbar-height", bar),
            ("--banner-height", note),
            ("--banner-row-height", row.unwrap_or_default()),
        ] {
            let _ = style.set_property(property, &format!("{height}px"));
        }
        let viewport = window()
            .inner_height()
            .ok()
            .and_then(|height| height.as_f64())
            .unwrap_or_default();
        let stuck = stuck(bar, note, row, viewport);
        let classes = root.class_list();
        for (class, on) in [
            ("topbar-loose", stuck == Stuck::Nothing),
            ("banner-row", stuck == Stuck::Row),
            ("banner-loose", matches!(stuck, Stuck::Bar | Stuck::Nothing)),
        ] {
            let _ = classes.toggle_with_force(class, on);
        }
    };
    let follow = Closure::<dyn FnMut()>::new(settle.clone());
    let Ok(observer) = web_sys::ResizeObserver::new(follow.as_ref().unchecked_ref()) else {
        return;
    };
    observer.observe(header);
    observer.observe(banner);
    // The bar and the banner live as long as the page, and so do the observer and its callback, and the listener for a
    // viewport that changes height (a phone turned, a zoom), which changes what may stay without changing either box.
    follow.forget();
    let _ = window_event_listener(ev::resize, move |_| settle());
}

#[component]
pub fn Topbar(deck: Deck, shell: Shell) -> impl IntoView {
    // The rules that keep focus clear of the top bar and the banner read their rendered heights.
    let header = NodeRef::<leptos::html::Header>::new();
    let banner = NodeRef::<leptos::html::P>::new();
    Effect::new(move |_| {
        if let (Some(header), Some(banner)) = (header.get(), banner.get()) {
            follow_heights(&header, &banner);
        }
    });
    let pending = deck.runner.pending();
    // A run in this tab says where it is, and how far its download is; the server's pull and the download can be
    // stopped.
    let in_tab = Memo::new(move |_| {
        let stage = deck.stage.get()?;
        let name = deck.run_model.get().unwrap_or_default();
        let size = deck.listed(&name).and_then(|m| m.x_browser);
        tab_note(&name, size, stage)
    });
    let bar = Memo::new(move |_| in_tab.with(|note| note.as_ref().and_then(|(_, bar)| *bar)));
    let stoppable = Memo::new(move |_| {
        matches!(
            deck.stage.get(),
            Some(Stage::Pulling | Stage::Downloading { .. })
        )
    });
    let held = Memo::new(move |_| deck.held());
    // Before a run of the picked "In browser" row that downloads its files: what it downloads.
    let download = move || {
        if pending.get() || !deck.in_browser.get() {
            return None;
        }
        let name = deck.model.get();
        let unkept = deck.kept.with(|kept| {
            kept.as_ref()
                .is_some_and(|(model, kept)| *model == name && !kept)
        });
        if !unkept {
            return None;
        }
        Some(first_download(
            &name,
            deck.browser_size()?,
            engine::keeps_files(),
        ))
    };
    // Whether the banner says anything: its sentence (`#run-note-text`) describes Run while there is one.
    let said =
        move || in_tab.with(Option::is_some) || held.with(Option::is_some) || download().is_some();
    // Stop leaves once the download is over; if it had the focus, which then went nowhere, Run takes it, as after Stop's
    // own press, rather than the page dropping it.
    let stop_focused = StoredValue::new(false);
    Effect::new(move |_| {
        if !stoppable.get() && stop_focused.get_value() {
            stop_focused.set_value(false);
            request_animation_frame(|| {
                let nowhere = document().active_element().is_none_or(|active| {
                    document()
                        .body()
                        .is_some_and(|body| body.is_same_node(Some(&active)))
                });
                if nowhere {
                    focus_later("run-key".to_string());
                }
            });
        }
    });
    view! {
        <header class="topbar" node_ref=header>
            <button
                type="button"
                id="sidebar-opener"
                class="button button-icon opener tip tip-below tip-start"
                aria-label="Open sidebar"
                aria-controls="sidebar"
                aria-expanded=move || shell.drawer.get().to_string()
                data-tip="Open sidebar\nCtrl+\\ or Cmd+\\"
                on:click=move |_| shell.open()
            >
                <Icon name="panel" />
            </button>
            <Logo class="topbar-logo" />
            <div class="topbar-crumb">
                <span class="topbar-crumb-text">"Playground"</span>
            </div>
            <div class="topbar-actions">
                <p class="changed" class:on=move || deck.stale.get() data-testid="changed">
                    {move || {
                        deck.stale
                            .get()
                            .then(|| view! { <span class="tag">"Inputs changed"</span> })
                    }}
                </p>
                <Share deck=deck />
                // `aria-disabled`, not `disabled`: a button that disables itself while focused drops focus to the page.
                // Its description is the banner's words alone, never the download bar's raw value.
                <button
                    type="button"
                    id="run-key"
                    class="button button-primary run-key tip tip-below tip-end"
                    aria-keyshortcuts="Control+Enter Meta+Enter"
                    aria-busy=move || pending.get().to_string()
                    aria-disabled=move || (!deck.can_run()).to_string()
                    aria-describedby=move || said().then_some("run-note-text")
                    data-tip="Send the state and questions\nCtrl+Enter or Cmd+Enter"
                    on:click=move |_| deck.run()
                >
                    <span class="run-face">
                        <span class="run-label" data-testid="run-label">
                            {move || match (pending.get(), deck.stage.get()) {
                                (false, _) => "Run",
                                (true, Some(Stage::Pulling)) => "Pulling",
                                (
                                    true,
                                    Some(Stage::Asking | Stage::Downloading { .. } | Stage::Starting),
                                ) => "Loading",
                                (true, Some(Stage::Running(_)) | None) => "Running",
                            }}
                        </span>
                        // Busy: the landing's block caret, blinking after the label (steady under reduced motion).
                        <Show when=move || pending.get()>
                            <span class="caret" aria-hidden="true"></span>
                        </Show>
                    </span>
                    // The widest face, unseen, in the same cell: the pill keeps one width while a run is in flight, as
                    // the landing's Copy pill does when it turns into Copied.
                    <span class="run-sizer" aria-hidden="true">
                        "Running"
                        <span class="caret"></span>
                    </span>
                </button>
            </div>
        </header>
        // A wait or a hint is quiet; only an error takes the fault colours. A run in this tab says where it is, with its
        // download bar and Stop, and stays under the top bar while in flight; a model this server does not run leads to
        // the ardana CLI's commands under Snippets (and, on a public server, to the browser default in this tab); before
        // a first run in this tab, what it downloads. The sentence is its own line of text (Run's description); the bar
        // and the actions beside it are not part of it.
        <p
            id="run-note"
            node_ref=banner
            class="banner"
            class:quiet=move || {
                deck.stage.with(Option::is_some) || deck.questions_error.with(Option::is_none)
            }
            class:busy=move || deck.stage.with(Option::is_some)
            data-testid="run-note"
        >
            <Show
                when=move || in_tab.with(Option::is_some)
                fallback=move || {
                    if let Some(held) = held.get() {
                        let command = matches!(held, Held::Pull(_) | Held::Cli(_))
                            .then(|| {
                                view! {
                                    <button
                                        type="button"
                                        class="banner-action"
                                        on:click=move |_| reveal("snippets")
                                    >
                                        "Show the command"
                                        <Icon name="arrow" />
                                    </button>
                                }
                            });
                        // A public server runs no model; its browser default runs in this tab instead. The pick puts
                        // the focus on Run, whose description then says what its first run downloads.
                        let instead = matches!(held, Held::Cli(_))
                            .then(|| deck.browser_default())
                            .flatten()
                            .map(|name| {
                                let label = format!("Run {name} in this tab instead");
                                view! {
                                    <button
                                        type="button"
                                        class="banner-action banner-instead"
                                        on:click=move |_| {
                                            deck.pick(&browser_value(&name));
                                            focus_later("run-key".to_string());
                                        }
                                    >
                                        {label}
                                    </button>
                                }
                            });
                        return Some(
                            view! {
                                <span id="run-note-text">{held.text()}</span>
                                <span class="banner-actions">{command} {instead}</span>
                            }
                                .into_any(),
                        );
                    }
                    download().map(|text| view! { <span id="run-note-text">{text}</span> }.into_any())
                }
            >
                // Each part updates in place, so a focused Stop keeps its focus while the bar moves. The bar and Stop
                // keep together, on a row of their own under the words on a phone, the part that stays under the top
                // bar when the whole banner would take too much of the screen.
                <span id="run-note-text">{move || in_tab.with(|note| note.as_ref().map(|(text, _)| text.clone()))}</span>
                <Show when=move || bar.with(Option::is_some) || stoppable.get()>
                    <span class="banner-track">
                        <Show when=move || bar.with(Option::is_some)>
                            <progress
                                class="banner-progress"
                                aria-hidden="true"
                                max=move || bar.get().map(|(_, total)| total.to_string())
                                value=move || bar.get().map(|(done, _)| done.to_string())
                            ></progress>
                        </Show>
                        <Show when=move || stoppable.get()>
                            <button
                                type="button"
                                class="banner-action banner-stop"
                                on:focus=move |_| stop_focused.set_value(true)
                                // Focus moving on to another control; Stop taken away sends it nowhere.
                                on:blur=move |event| {
                                    if event.related_target().is_some() {
                                        stop_focused.set_value(false);
                                    }
                                }
                                on:click=move |_| {
                                    deck.stop();
                                    focus_later("run-key".to_string());
                                }
                            >
                                "Stop"
                            </button>
                        </Show>
                    </span>
                </Show>
            </Show>
        </p>
    }
}

/// Share: a link (Jev's link format) to the editors and the picked model, in a popover under the button.
#[component]
fn Share(deck: Deck) -> impl IntoView {
    let open = RwSignal::new(false);
    let origin = window().location().origin().unwrap_or_default();
    let link = move || {
        let payload = SharePayload::new(
            deck.state_text.get(),
            deck.questions_text.get(),
            deck.model.get(),
        );
        share::link(&origin, &payload)
    };
    let link = Memo::new(move |_| open.get().then(&link).unwrap_or_default());
    // Whether `target` is the popover, its button or anything in them.
    let within = |target: Option<web_sys::EventTarget>| {
        document().get_element_by_id("share").is_some_and(|share| {
            target
                .and_then(|target| target.dyn_into::<web_sys::Node>().ok())
                .is_some_and(|node| share.contains(Some(&node)))
        })
    };
    // A click anywhere outside the popover and its button closes it.
    let outside = window_event_listener(ev::click, move |event| {
        if open.get_untracked() && !within(event.target()) {
            open.set(false);
        }
    });
    // Escape closes it wherever the focus is (after a click on its words, the page has it), and gives Share the focus.
    let escape = window_event_listener(ev::keydown, move |event| {
        if event.key() == "Escape" && open.get_untracked() {
            open.set(false);
            focus_later("share-button".to_string());
        }
    });
    on_cleanup(move || {
        outside.remove();
        escape.remove();
    });
    let toggle = move |_| {
        let now_open = !open.get_untracked();
        open.set(now_open);
        if now_open {
            focus_later("share-link".to_string());
        }
    };
    view! {
        // Focus that moves on beyond the popover and its button (Tab out of it) closes it; focus that goes nowhere (a
        // click on its words) leaves it open.
        <div
            class="share"
            id="share"
            on:focusout=move |event| {
                let to = event.related_target();
                if to.is_some() && !within(to) {
                    open.set(false);
                }
            }
        >
            <button
                type="button"
                id="share-button"
                class="button"
                aria-label="Share link"
                aria-expanded=move || open.get().to_string()
                // The popover exists only while open: name it only then.
                aria-controls=move || open.get().then_some("share-panel")
                on:click=toggle
            >
                <Icon name="link" class="share-icon" />
                <span class="share-label">"Share"</span>
            </button>
            <Show when=move || open.get()>
                <div class="popover" id="share-panel">
                    <label class="field-label" for="share-link">"Link to these inputs"</label>
                    <div class="popover-row">
                        <input
                            id="share-link"
                            class="field-input field-code"
                            type="text"
                            readonly
                            spellcheck="false"
                            prop:value=move || link.get()
                            on:focus=move |event| {
                                if let Some(input) = event
                                    .target()
                                    .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                {
                                    input.select();
                                }
                            }
                        />
                        <CopyKey what="share link" text=move || link.get() />
                    </div>
                    <p class="field-note">"Anyone with this link opens these inputs and this model on their own Ardana."</p>
                </div>
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Backend;

    #[test]
    fn the_banner_says_where_a_run_in_the_tab_is() {
        let note = |stage| tab_note("decider-0.8b", Some(467_748_928), stage);
        assert_eq!(
            note(Stage::Asking),
            None,
            "a moment's wait for the server says nothing"
        );
        assert_eq!(
            note(Stage::Pulling),
            Some((
                "Pulling the browser files of decider-0.8b (467.7 MB) on the server".to_string(),
                None
            ))
        );
        let downloading = Stage::Downloading {
            done: 118_400_000,
            total: 467_748_928,
        };
        assert_eq!(
            note(downloading),
            Some((
                "Downloading decider-0.8b into this tab: 118.4 MB of 467.7 MB".to_string(),
                Some((118_400_000, 467_748_928))
            ))
        );
        assert_eq!(
            note(Stage::Starting).map(|(text, _)| text).as_deref(),
            Some("Starting decider-0.8b in this tab")
        );
        assert_eq!(
            note(Stage::Running(Backend::Wasm))
                .map(|(text, _)| text)
                .as_deref(),
            Some("Running decider-0.8b in this tab on WASM")
        );
        assert_eq!(
            tab_note("decider-0.8b", None, Stage::Pulling)
                .map(|(text, _)| text)
                .as_deref(),
            Some("Pulling the browser files of decider-0.8b on the server")
        );
    }

    /// The status region hears the banner's words, a download in tenths.
    #[test]
    fn the_status_counts_a_download_in_tenths() {
        let status = |stage| tab_status("decider-0.8b", Some(467_748_928), stage);
        let at = |done| Stage::Downloading {
            done,
            total: 467_748_928,
        };
        assert_eq!(
            status(at(0)).as_deref(),
            Some("Downloading decider-0.8b into this tab: 0% of 467.7 MB")
        );
        assert_eq!(status(at(46_774_892)), status(at(0)));
        assert_eq!(
            status(at(46_774_893)).as_deref(),
            Some("Downloading decider-0.8b into this tab: 10% of 467.7 MB")
        );
        assert_eq!(
            status(at(467_748_928)).as_deref(),
            Some("Downloading decider-0.8b into this tab: 100% of 467.7 MB")
        );
        assert_eq!(
            status(Stage::Running(Backend::WebGpu)).as_deref(),
            Some("Running decider-0.8b in this tab on WebGPU")
        );
        assert_eq!(status(Stage::Asking), None);
    }

    /// The bars keep to a third of the viewport: the whole banner on a phone at its own text size, its last row with
    /// enlarged text, nothing of it in landscape with enlarged text, and nothing at all at 400% zoom with enlarged text.
    #[test]
    fn what_stays_holds_a_third_of_the_viewport() {
        assert_eq!(stuck(70.0, 90.0, Some(48.0), 844.0), Stuck::Banner);
        assert_eq!(stuck(113.0, 248.0, Some(52.0), 640.0), Stuck::Row);
        assert_eq!(stuck(126.0, 100.0, None, 390.0), Stuck::Bar);
        assert_eq!(stuck(70.0, 90.0, Some(48.0), 256.0), Stuck::Bar);
        assert_eq!(stuck(113.0, 248.0, Some(52.0), 256.0), Stuck::Nothing);
        // A banner with nothing to say takes no room.
        assert_eq!(stuck(70.0, 0.0, None, 256.0), Stuck::Banner);
        assert_eq!(stuck(85.0, 0.0, None, 255.0), Stuck::Banner);
        assert_eq!(stuck(86.0, 0.0, None, 255.0), Stuck::Nothing);
    }

    #[test]
    fn a_first_run_in_the_tab_says_what_it_downloads_and_takes() {
        assert_eq!(
            first_download("decider-0.8b", 467_748_928, true),
            "Run downloads decider-0.8b into this tab once: 467.7 MB, kept by this browser. Running it takes 3 to 4 \
             times that in memory."
        );
        assert_eq!(
            first_download("decider-0.8b", 467_748_928, false),
            "Run downloads decider-0.8b into this tab on each visit: 467.7 MB, as a page without HTTPS keeps no \
             files. Running it takes 3 to 4 times that in memory."
        );
    }
}

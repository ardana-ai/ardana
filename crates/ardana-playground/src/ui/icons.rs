//! The drawn glyphs: one 16-unit grid, one stroke, current colour. Every icon is decorative (`aria-hidden`); the text
//! or `aria-label` beside it carries the meaning.

use leptos::prelude::*;

/// A glyph by name; unknown names draw nothing.
#[component]
pub fn Icon(name: &'static str, #[prop(optional)] class: &'static str) -> impl IntoView {
    let (filled, d) = path(name);
    let class = if filled {
        format!("icon icon-filled {class}")
    } else {
        format!("icon {class}")
    };
    view! {
        <svg class=class viewBox="0 0 16 16" aria-hidden="true" focusable="false">
            <path d=d />
        </svg>
    }
}

/// Whether the glyph is filled, and its path.
fn path(name: &str) -> (bool, &'static str) {
    match name {
        "chevrons-left" => (false, "M9 3 4 8l5 5M13 3 8 8l5 5"),
        "chevrons-right" => (false, "M7 3l5 5-5 5M3 3l5 5-5 5"),
        "page" => (
            false,
            "M4 1.5h5.5L13 5v9.5H4zM9.5 1.5V5H13M6.5 8.5h3M6.5 11h3",
        ),
        "check" => (false, "M3 8.5l3 3 7-7"),
        "plus" => (false, "M8 3v10M3 8h10"),
        "trash" => (false, "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.7 8.5h5.6l.7-8.5"),
        "copy" => (false, "M6 6h7v7H6zM3 10V3h7"),
        "undo" => (false, "M3.5 6.5h6a3 3 0 0 1 0 6H6M3.5 6.5 6 4M3.5 6.5 6 9"),
        "triangle" => (true, "M5.5 3.5v9l6-4.5z"),
        "chevron-down" => (false, "M4 6.5l4 4 4-4"),
        "hash" => (false, "M6 2.5 4.5 13.5M11.5 2.5 10 13.5M2.5 6h11M2.5 10h11"),
        "text" => (false, "M2.5 4.5h11M2.5 8h11M2.5 11.5h7"),
        "alert" => (
            false,
            "M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM8 5v3.5M8 11v.5",
        ),
        "info" => (
            false,
            "M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM8 7.5V11M8 5v.5",
        ),
        "target" => (
            false,
            "M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM8 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
        ),
        "gauge" => (false, "M2.5 11.5a5.5 5.5 0 0 1 11 0M8 11.5l2.5-4"),
        "braces" => (
            false,
            "M5.5 2.5c-1.5 0-2 .5-2 2v2c0 1-.5 1.5-1.5 1.5 1 0 1.5.5 1.5 1.5v2c0 1.5.5 2 2 2M10.5 2.5c1.5 0 2 .5 2 2v2c0 1 .5 1.5 1.5 1.5-1 0-1.5.5-1.5 1.5v2c0 1.5-.5 2-2 2",
        ),
        "code" => (false, "M5 4 1.5 8 5 12M11 4l3.5 4L11 12"),
        "link" => (
            false,
            "M6.5 9.5l3-3M5 7 3.5 8.5a2.5 2.5 0 0 0 3.5 3.5L8.5 10.5M11 9l1.5-1.5a2.5 2.5 0 0 0-3.5-3.5L7.5 5.5",
        ),
        "spinner" => (false, "M8 1.5a6.5 6.5 0 0 1 6.5 6.5"),
        _ => (false, ""),
    }
}

//! The drawn glyphs, in ardana.ai's icon style: one 16-unit grid, one round stroke, current colour (the copy, check
//! and arrow glyphs are the landing's own). Every icon is decorative (`aria-hidden`); the text or `aria-label` beside
//! it carries the meaning.

use leptos::prelude::*;

/// A glyph by name; unknown names draw nothing.
#[component]
pub fn Icon(name: &'static str, #[prop(optional)] class: &'static str) -> impl IntoView {
    view! {
        <svg class=format!("icon {class}") viewBox="0 0 16 16" aria-hidden="true" focusable="false">
            <path d=path(name) />
        </svg>
    }
}

/// The glyph's path.
fn path(name: &str) -> &'static str {
    match name {
        // A window with its sidebar: the sidebar's opener and closer.
        "panel" => {
            "M3 2.75h10A1.25 1.25 0 0 1 14.25 4v8A1.25 1.25 0 0 1 13 13.25H3A1.25 1.25 0 0 1 1.75 12V4A1.25 1.25 0 0 1 3 2.75zM6.25 2.75v10.5"
        }
        "check" => "M3.25 8.5l3 3 6.5-7",
        "plus" => "M8 3v10M3 8h10",
        "trash" => "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.7 8.5h5.6l.7-8.5",
        // The landing's copy glyph: its 8x8 rounded square and the corner of the one behind it.
        "copy" => {
            "M7 5.25h4.5A1.75 1.75 0 0 1 13.25 7v4.5a1.75 1.75 0 0 1-1.75 1.75H7a1.75 1.75 0 0 1-1.75-1.75V7A1.75 1.75 0 0 1 7 5.25zM10.75 2.75h-6a2 2 0 0 0-2 2v6"
        }
        "arrow" => "M3 8h9.5M8.5 4l4 4-4 4",
        "link" => {
            "M6.5 9.5l3-3M5 7 3.5 8.5a2.5 2.5 0 0 0 3.5 3.5L8.5 10.5M11 9l1.5-1.5a2.5 2.5 0 0 0-3.5-3.5L7.5 5.5"
        }
        "chevron-down" => "M4 6.5l4 4 4-4",
        "chevron-right" => "M6.5 4l4 4-4 4",
        "alert" => "M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM8 5v3.5M8 11v.5",
        "info" => "M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM8 7.5V11M8 5v.5",
        _ => "",
    }
}

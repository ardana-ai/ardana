//! A value the API returned, shown formatted while `data-value` keeps it raw and `data-field` names where it sits in
//! the response (a JSON pointer), so every displayed figure can be checked against the response it came from.

use leptos::prelude::*;

use crate::format;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Probabilities: percent with one decimal.
    Percent,
    /// Confidence, noul, score and other extras: two decimals.
    Fixed2,
}

impl Format {
    fn name(self) -> &'static str {
        match self {
            Format::Percent => "percent",
            Format::Fixed2 => "fixed2",
        }
    }

    fn apply(self, value: f64) -> String {
        match self {
            Format::Percent => format::percent(value),
            Format::Fixed2 => format::fixed2(value),
        }
    }
}

/// A JSON pointer from its unescaped parts.
pub fn pointer<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    parts
        .into_iter()
        .map(|part| format!("/{}", part.replace('~', "~0").replace('/', "~1")))
        .collect()
}

/// A number from the response.
#[component]
pub fn Number(
    field: String,
    value: f64,
    format: Format,
    #[prop(into)] class: String,
) -> impl IntoView {
    view! {
        <span class=class data-field=field data-value=format::raw(value) data-format=format.name()>
            {matrix(&format.apply(value))}
        </span>
    }
}

/// Figure text for the dot-matrix face, whose own full stop is a cross of dots that reads as `+`: each `.` is set
/// in its own span (`.matrix-point`), so the text stays the same and the decimal point reads as one.
pub fn matrix(text: &str) -> impl IntoView + use<> {
    let mut parts = text.split('.');
    let first = parts.next().unwrap_or_default().to_string();
    let rest: Vec<String> = parts.map(str::to_string).collect();
    view! {
        {first}
        {rest
            .into_iter()
            .map(|part| view! { <span class="matrix-point">"."</span> {part} })
            .collect_view()}
    }
}

/// A count or a name from the response, shown as it is.
#[component]
pub fn Verbatim(field: String, value: String, #[prop(into)] class: String) -> impl IntoView {
    let raw = value.clone();
    view! {
        <span class=class data-field=field data-value=raw data-format="verbatim">
            {value}
        </span>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointers_escape_slash_and_tilde() {
        assert_eq!(
            pointer(["answers", "a/b~c", "noul"]),
            "/answers/a~1b~0c/noul"
        );
    }
}

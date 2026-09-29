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
            {format.apply(value)}
        </span>
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

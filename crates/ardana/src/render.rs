//! How the commands print for people: answers, tables and sectioned model information. Figures use the playground's
//! display rules (`ardana_api::format`): probabilities as percent with one decimal, everything else with two.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ardana_api::format::{fixed2, percent};
use ardana_api::{Answer, SystemOneRequest, SystemOneResponse};
use serde_json::Value;

/// Each answer under its question, the answer marked `*`, its figures on the line below.
pub fn answers(request: &SystemOneRequest, response: &SystemOneResponse) -> String {
    let blocks: Vec<String> = response
        .answers
        .iter()
        .map(|(id, answer)| {
            let title = request
                .questions
                .get(id)
                .and_then(|spec| spec.get("instructions"))
                .map(text)
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| id.clone());
            let (rows, figures) = match answer {
                Answer::Choice(a) => {
                    let rows = a
                        .probabilities
                        .iter()
                        .map(|(name, p)| Row::new(*name == a.choice, name, percent(*p)))
                        .collect();
                    let mut figures = vec![format!("confidence {}", fixed2(a.confidence))];
                    figures.extend(a.x_p_max.map(|v| format!("p max {}", percent(v))));
                    figures.extend(a.x_certainty.map(|v| format!("certainty {}", fixed2(v))));
                    (rows, figures)
                }
                Answer::Score(a) => {
                    let top = a.probabilities.values().copied().fold(f64::MIN, f64::max);
                    let rows = a
                        .probabilities
                        .iter()
                        .map(|(level, p)| {
                            let name = match a.legend.get(level) {
                                Some(legend) if !legend.is_empty() => format!("{level}  {legend}"),
                                _ => level.clone(),
                            };
                            let mut row = Row::new(*p == top, &name, percent(*p));
                            if let Some(fit) = a.x_level_fit.as_ref().and_then(|f| f.get(level)) {
                                row.note = format!("fit {}", percent(*fit));
                            }
                            row
                        })
                        .collect();
                    let mut figures = vec![
                        format!("score {}", fixed2(a.score)),
                        format!("confidence {}", fixed2(a.confidence)),
                    ];
                    figures.extend(a.x_p_max.map(|v| format!("p max {}", percent(v))));
                    figures.extend(a.x_certainty.map(|v| format!("certainty {}", fixed2(v))));
                    figures.extend(a.x_fit_mass.map(|v| format!("fit mass {}", fixed2(v))));
                    (rows, figures)
                }
                Answer::Noul(a) => (
                    vec![Row::new(false, "probability of yes", fixed2(a.noul))],
                    Vec::new(),
                ),
            };
            let mut block = format!("{title}\n{}", rows_text(&rows));
            if !figures.is_empty() {
                block.push_str(&format!("    {}\n", figures.join(", ")));
            }
            block
        })
        .collect();
    blocks.join("\n")
}

/// One option or level: whether it is the answer, its name, its figure and an optional note.
struct Row {
    answer: bool,
    name: String,
    value: String,
    note: String,
}

impl Row {
    fn new(answer: bool, name: &str, value: String) -> Row {
        Row {
            answer,
            name: name.to_string(),
            value,
            note: String::new(),
        }
    }
}

fn rows_text(rows: &[Row]) -> String {
    let name_width = rows
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0);
    let value_width = rows
        .iter()
        .map(|r| r.value.chars().count())
        .max()
        .unwrap_or(0);
    rows.iter()
        .map(|r| {
            let mark = if r.answer { '*' } else { ' ' };
            let line = format!(
                "  {mark} {:<name_width$}  {:>value_width$}  {}",
                r.name, r.value, r.note
            );
            format!("{}\n", line.trim_end())
        })
        .collect()
}

/// A JSON value as a person reads it: a string as is, anything else as compact JSON.
fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// What `--verbose` adds: the model, the token counts and the timings.
pub fn verbose(
    response: &SystemOneResponse,
    load: Duration,
    answer: Duration,
    total: Duration,
) -> String {
    let rows = [
        ("model", response.model.clone()),
        ("input tokens", response.usage.input_tokens.to_string()),
        ("output tokens", response.usage.output_tokens.to_string()),
        ("load duration", duration(load)),
        ("answer duration", duration(answer)),
        ("total duration", duration(total)),
    ];
    let width = rows
        .iter()
        .map(|(name, _)| name.len() + 1)
        .max()
        .unwrap_or(0);
    rows.iter()
        .map(|(name, value)| format!("{:<width$} {value}\n", format!("{name}:")))
        .collect()
}

/// `842ms` under a second, else `1.05s`.
pub fn duration(d: Duration) -> String {
    if d < Duration::from_secs(1) {
        format!("{}ms", d.as_millis())
    } else {
        format!("{:.2}s", d.as_secs_f64())
    }
}

/// Columns padded to their widest cell, two spaces apart, like `ollama list`.
pub fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..header.len())
        .map(|i| {
            rows.iter()
                .map(|row| row.get(i).map_or(0, |cell| cell.chars().count()))
                .chain(std::iter::once(header[i].len()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: Vec<&str>| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, &width)| format!("{cell:<width$}"))
            .collect();
        format!("{}\n", padded.join("  ").trim_end())
    };
    let mut out = line(header.to_vec());
    for row in rows {
        out.push_str(&line(row.iter().map(String::as_str).collect()));
    }
    out
}

/// Titled sections of name-value rows, indented like `ollama show`.
pub fn sections(sections: &[(&str, Vec<(&str, String)>)]) -> String {
    let width = sections
        .iter()
        .flat_map(|(_, rows)| rows.iter().map(|(name, _)| name.len()))
        .max()
        .unwrap_or(0);
    let blocks: Vec<String> = sections
        .iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(title, rows)| {
            let mut block = format!("  {title}\n");
            for (name, value) in rows {
                block.push_str(&format!("    {name:<width$}    {value}\n"));
            }
            block
        })
        .collect();
    blocks.join("\n")
}

/// A `YYYY-MM-DD` date relative to `today` (days since 1970-01-01): `today`, `yesterday`, `3 days ago`, `2 months ago`.
pub fn ago(date: &str, today: i64) -> String {
    let Some(days) = days(date).map(|d| today - d) else {
        return date.to_string();
    };
    match days {
        ..=0 => "today".to_string(),
        1 => "yesterday".to_string(),
        2..=59 => format!("{days} days ago"),
        60..=729 => format!("{} months ago", days / 30),
        _ => format!("{} years ago", days / 365),
    }
}

/// Today, UTC, as days since 1970-01-01 (the registry dates pulls in UTC).
pub fn today() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    i64::try_from(secs / 86_400).unwrap_or(0)
}

/// Days since 1970-01-01 of a `YYYY-MM-DD` date (Howard Hinnant's `days_from_civil`).
fn days(date: &str) -> Option<i64> {
    let mut parts = date.splitn(3, '-').map(str::parse::<i64>);
    let (y, m, d) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn answers_read_like_the_playground() {
        let request: SystemOneRequest = serde_json::from_value(json!({
            "state": "My card was charged twice.",
            "questions": {
                "department": { "type": "choice", "instructions": "Which department?", "criteria": ["billing", "sales"] },
                "refund": { "type": "noul", "instructions": "Does the customer ask for a refund?" },
                "mood": { "type": "score", "criteria": ["calm", "upset"] }
            }
        }))
        .unwrap();
        let response: SystemOneResponse = serde_json::from_value(json!({
            "model": "decider-2b-v11",
            "answers": {
                "department": { "type": "choice", "choice": "billing", "confidence": 0.9019,
                    "probabilities": { "billing": 0.9346, "sales": 0.0654 }, "x_p_max": 0.9346, "x_certainty": 0.7643 },
                "refund": { "type": "noul", "noul": 0.9261 },
                "mood": { "type": "score", "score": 0.31, "confidence": 0.38,
                    "legend": { "0": "calm", "1": "upset" }, "probabilities": { "0": 0.69, "1": 0.31 } }
            },
            "usage": { "input_tokens": 72, "output_tokens": 0 }
        }))
        .unwrap();
        assert_eq!(
            answers(&request, &response),
            "Which department?
  * billing  93.5%
    sales     6.5%
    confidence 0.90, p max 93.5%, certainty 0.76

Does the customer ask for a refund?
    probability of yes  0.93

mood
  * 0  calm   69.0%
    1  upset  31.0%
    score 0.31, confidence 0.38
"
        );
    }

    #[test]
    fn tables_and_sections_align() {
        let rows = vec![vec!["decider:2b".to_string(), "1.3 GB".to_string()]];
        assert_eq!(
            table(&["NAME", "SIZE"], &rows),
            "NAME        SIZE\ndecider:2b  1.3 GB\n"
        );
        assert_eq!(table(&["NAME", "SIZE"], &[]), "NAME  SIZE\n");
        let shown = sections(&[
            ("Model", vec![("name", "decider:2b".to_string())]),
            ("Empty", vec![]),
            ("Files", vec![("weights", "/w.gguf".to_string())]),
        ]);
        assert_eq!(
            shown,
            "  Model\n    name       decider:2b\n\n  Files\n    weights    /w.gguf\n"
        );
    }

    #[test]
    fn dates_read_relative_to_today() {
        let today = days("2026-09-29").unwrap();
        assert_eq!(days("1970-01-01"), Some(0));
        assert_eq!(days("2000-02-29"), Some(11_016));
        assert_eq!(ago("2026-09-29", today), "today");
        assert_eq!(ago("2026-09-28", today), "yesterday");
        assert_eq!(ago("2026-09-19", today), "10 days ago");
        assert_eq!(ago("2026-03-01", today), "7 months ago");
        assert_eq!(ago("2023-09-29", today), "3 years ago");
        assert_eq!(ago("not a date", today), "not a date");
    }

    #[test]
    fn durations_switch_to_seconds() {
        assert_eq!(duration(Duration::from_millis(842)), "842ms");
        assert_eq!(duration(Duration::from_millis(1049)), "1.05s");
    }
}

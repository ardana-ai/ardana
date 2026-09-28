//! The chat layout of stock instruct models: the head and tail ids a tokenizer's chat template wraps one user turn
//! with, derived as decider 1.6.0 derives them (`decider/prompt.py#ChatTemplate`), plus its two start-up checks.
//!
//! The template is rendered for one user message holding [`SENTINEL`] with `add_generation_prompt`, the way
//! transformers' `apply_chat_template` renders it (`trim_blocks`, `lstrip_blocks`, loop controls, Python string
//! methods, `raise_exception`, `strftime_now`), and split at the sentinel. `strftime_now` formats the fixed date
//! [`TEMPLATE_DATE`] instead of the clock, so every head is the same on every day.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use minijinja::{Environment, ErrorKind, Value};
use serde_json::Value as Json;
use tokenizers::Tokenizer;

use crate::profile::Layout;
use crate::prompt::{self, ANSWER, LabelTable, NARROW, PromptError};

/// The user message the template is rendered with; head and tail are the text before and after it.
pub const SENTINEL: &str = "@@DECIDER_USER_CONTENT@@";

/// The template file transformers prefers over `tokenizer_config.json#chat_template`.
pub const TEMPLATE_FILE: &str = "chat_template.jinja";
/// The tokenizer config holding the special tokens and, without [`TEMPLATE_FILE`], the template.
pub const CONFIG_FILE: &str = "tokenizer_config.json";

/// The date `strftime_now` formats: 26 July 2024, the date Llama 3.2's template falls back to without the function.
pub const TEMPLATE_DATE: (u32, u32, u32) = (2024, 7, 26);

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Thinking blocks a template leaves open at the end of the generation prompt, and how decider closes them.
const CLOSE_THINKING: [(&str, &str); 3] = [
    ("<think>\n", "</think>\n\n"),
    ("<think>", "</think>\n\n"),
    ("<|channel>thought\n", "<channel|>"),
];

/// The special tokens a template may print, from `tokenizer_config.json`; `None` leaves the variable undefined.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TemplateSpecials {
    pub bos_token: Option<String>,
    pub eos_token: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChatTemplateError {
    #[error(
        "{} has no chat template: neither {TEMPLATE_FILE} nor a \"chat_template\" in {CONFIG_FILE}",
        .dir.display()
    )]
    Missing { dir: PathBuf },
    #[error("reading {}: {err}", .path.display())]
    Read { path: PathBuf, err: std::io::Error },
    #[error("{}: {msg}", .path.display())]
    Config { path: PathBuf, msg: String },
    #[error("rendering the chat template: {0}")]
    Render(minijinja::Error),
    #[error("the rendered chat template holds the user message {0} times, not once")]
    Sentinel(usize),
    #[error("{0}")]
    Prompt(PromptError),
    #[error(
        "labels_single_token: the label {label:?} does not tokenize as one token after \"{ANSWER}: (\""
    )]
    LabelsSingleToken { label: String },
    #[error("tail_boundary: the chat template tail merges with the answer prefix \"{ANSWER}: (\"")]
    TailBoundary,
}

impl From<PromptError> for ChatTemplateError {
    fn from(err: PromptError) -> Self {
        ChatTemplateError::Prompt(err)
    }
}

/// The chat template and special tokens of the tokenizer in `dir`: `chat_template.jinja`, else the `chat_template`
/// string of `tokenizer_config.json` (transformers' precedence).
pub fn read_template(dir: &Path) -> Result<(String, TemplateSpecials), ChatTemplateError> {
    let config_path = dir.join(CONFIG_FILE);
    let config =
        match std::fs::read_to_string(&config_path) {
            Ok(text) => Some(serde_json::from_str::<Json>(&text).map_err(|err| {
                ChatTemplateError::Config {
                    path: config_path.clone(),
                    msg: err.to_string(),
                }
            })?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(ChatTemplateError::Read {
                    path: config_path,
                    err,
                });
            }
        };
    let config = config.as_ref();
    let token = |key: &str| match config.and_then(|c| c.get(key)) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(ChatTemplateError::Config {
            path: config_path.clone(),
            msg: format!("\"{key}\" must be a string, got {other}"),
        }),
    };
    let specials = TemplateSpecials {
        bos_token: token("bos_token")?,
        eos_token: token("eos_token")?,
    };

    let template_path = dir.join(TEMPLATE_FILE);
    let template = match std::fs::read_to_string(&template_path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            match config.and_then(|c| c.get("chat_template")) {
                None | Some(Json::Null) => {
                    return Err(ChatTemplateError::Missing {
                        dir: dir.to_path_buf(),
                    });
                }
                Some(Json::String(s)) => s.clone(),
                Some(_) => {
                    return Err(ChatTemplateError::Config {
                        path: config_path,
                        msg: "\"chat_template\" must be one template string, named templates are not supported"
                            .into(),
                    });
                }
            }
        }
        Err(err) => {
            return Err(ChatTemplateError::Read {
                path: template_path,
                err,
            });
        }
    };
    if template.is_empty() {
        return Err(ChatTemplateError::Missing {
            dir: dir.to_path_buf(),
        });
    }
    Ok((template, specials))
}

/// The chat layout of `template` on `tokenizer`: head and tail ids around one user turn, after the start-up checks
/// `labels_single_token` (`A`..`J` stay single tokens after `Answer: (`) and `tail_boundary` (the tail does not merge
/// with `Answer: (`).
pub fn chat_layout(
    template: &str,
    tokenizer: &Tokenizer,
    specials: &TemplateSpecials,
) -> Result<Layout, ChatTemplateError> {
    let rendered = render(template, specials)?;
    let parts: Vec<&str> = rendered.split(SENTINEL).collect();
    let [head_text, tail_text] = parts[..] else {
        return Err(ChatTemplateError::Sentinel(parts.len() - 1));
    };
    let mut tail_text = tail_text.to_string();
    for (opened, closed) in CLOSE_THINKING {
        if tail_text.ends_with(opened) {
            tail_text.push_str(closed);
        }
    }
    let head = prompt::encode(tokenizer, head_text)?;
    let tail = prompt::encode(tokenizer, &tail_text)?;

    let labels = LabelTable::new(tokenizer)?;
    let pre = prompt::chat_answer_ids(tokenizer, 0, false)?;
    for (name, &id) in labels.names.iter().zip(&labels.ids).take(NARROW) {
        let ids = prompt::encode(tokenizer, &format!("{ANSWER}: ({name}"))?;
        if ids.split_last() != Some((&id, &pre[..])) {
            return Err(ChatTemplateError::LabelsSingleToken {
                label: name.clone(),
            });
        }
    }
    let joined = prompt::encode(tokenizer, &format!("{tail_text}{ANSWER}: ("))?;
    if joined[..] != [&tail[..], &pre[..]].concat()[..] {
        return Err(ChatTemplateError::TailBoundary);
    }
    Ok(Layout::Chat { head, tail })
}

/// `apply_chat_template([{"role": "user", "content": SENTINEL}], add_generation_prompt=True)`, with
/// `enable_thinking=False` only when the template mentions it (decider).
fn render(template: &str, specials: &TemplateSpecials) -> Result<String, ChatTemplateError> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
    env.add_function(
        "raise_exception",
        |msg: String| -> Result<Value, minijinja::Error> {
            Err(minijinja::Error::new(ErrorKind::InvalidOperation, msg))
        },
    );

    env.add_function("strftime_now", |format: String| strftime(&format));

    let user = BTreeMap::from([("role", "user"), ("content", SENTINEL)]);
    let mut ctx = BTreeMap::from([
        ("messages", Value::from_serialize([user])),
        ("tools", Value::from(())),
        ("documents", Value::from(())),
        ("add_generation_prompt", Value::from(true)),
    ]);
    for (key, token) in [
        ("bos_token", &specials.bos_token),
        ("eos_token", &specials.eos_token),
    ] {
        if let Some(token) = token {
            ctx.insert(key, Value::from(token.as_str()));
        }
    }
    if template.contains("enable_thinking") {
        ctx.insert("enable_thinking", Value::from(false));
    }
    env.render_str(&without_generation_tags(template), ctx)
        .map_err(ChatTemplateError::Render)
}

/// Python's `strftime` on [`TEMPLATE_DATE`] for the directives chat templates use: `%d`, `%b`, `%B`, `%Y`, `%%`.
/// Any other directive is an error naming it.
fn strftime(format: &str) -> Result<String, minijinja::Error> {
    let (year, month, day) = TEMPLATE_DATE;
    let name = MONTHS[month as usize - 1];
    let mut out = String::with_capacity(format.len() + 8);
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('d') => out.push_str(&format!("{day:02}")),
            Some('b') => out.push_str(&name[..3]),
            Some('B') => out.push_str(name),
            Some('Y') => out.push_str(&year.to_string()),
            Some('%') => out.push('%'),
            other => {
                let directive = other.map_or_else(|| "%".to_string(), |d| format!("%{d}"));
                return Err(minijinja::Error::new(
                    ErrorKind::InvalidOperation,
                    format!("strftime_now: unsupported directive {directive:?} in {format:?}"),
                ));
            }
        }
    }
    Ok(out)
}

/// transformers' `{% generation %}` .. `{% endgeneration %}` extension only marks assistant text and renders its
/// body unchanged; minijinja has no such tag, so each becomes `{% if true %}` .. `{% endif %}` with its whitespace
/// control kept.
fn without_generation_tags(template: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{%") {
        let Some(len) = rest[start..].find("%}") else {
            break;
        };
        let tag = &rest[start..start + len + 2];
        let inner =
            tag[2..tag.len() - 2].trim_matches(|c: char| c == '-' || c == '+' || c.is_whitespace());
        out.push_str(&rest[..start]);
        match inner {
            "generation" => out.push_str(&tag.replacen("generation", "if true", 1)),
            "endgeneration" => out.push_str(&tag.replacen("endgeneration", "endif", 1)),
            _ => out.push_str(tag),
        }
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_tags_become_if_blocks() {
        let t = "a{% generation %}b{%- endgeneration -%}c{% if x %}{% endif %}";
        assert_eq!(
            without_generation_tags(t),
            "a{% if true %}b{%- endif -%}c{% if x %}{% endif %}"
        );
    }

    #[test]
    fn strftime_formats_the_template_date() {
        assert_eq!(strftime("%d %b %Y").unwrap(), "26 Jul 2024");
        assert_eq!(strftime("%d %B %Y").unwrap(), "26 July 2024");
        assert_eq!(strftime("100%%").unwrap(), "100%");
        let err = strftime("%H:%M").unwrap_err().to_string();
        assert!(err.contains("\"%H\""), "{err}");
    }

    #[test]
    fn raise_exception_fails_the_render() {
        let err = render(
            "{{ raise_exception('bad role') }}",
            &TemplateSpecials::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("bad role"), "{err}");
    }
}

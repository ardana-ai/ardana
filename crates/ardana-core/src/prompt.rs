//! Prompt rows, token for token as decider 1.6.0 builds them (`decider/prompt.py`, `decider/prompt_fast.py`).
//!
//! Plain layout, one row:
//!
//! ```text
//! Context:\n<state>\n\nQuestion: <q>\nOptions:\n(A) <opt>\n(B) <opt>\nAnswer: (
//! ```
//!
//! With several questions in a row they are numbered (`Question 1:`, `Answer 1: (`). Up to [`NARROW`] options are
//! rendered as one string; wider questions (up to [`MAX_OPTIONS`]) are assembled from ids so every label is a single
//! token (`"\n("`, the label token, `") <opt>"`). The answer slot is the final `" ("` token of each `Answer: (`.
//!
//! Chat layout (`Layout::Chat`): the template head, the context and every question block (header and options encoded
//! apart) form the user turn, then the template tail and the answer pieces follow.

use tokenizers::Tokenizer;

use crate::profile::Layout;

/// Letters of the narrow rendering.
pub const LETTERS: &str = "ABCDEFGHIJ";
/// Questions with at most this many options use the narrow `"(A) .. (J)"` string rendering.
pub const NARROW: usize = 10;
/// Width of the label head: `A`..`Z`, then the two-letter labels that are single tokens.
pub const MAX_OPTIONS: usize = 255;

/// The text of the chat answer piece before `": ("`.
const ANSWER: &str = "Answer";

#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    #[error("tokenizer: {0}")]
    Tokenizer(String),
    #[error("2..{MAX_OPTIONS} options required")]
    Options,
    #[error(
        "the tokenizer has {found} single-token labels among A..Z and AA..ZZ, {MAX_OPTIONS} are needed"
    )]
    Labels { found: usize },
}

/// Token ids of `text` without special tokens (`tok.encode(text, add_special_tokens=False)`).
pub fn encode(tok: &Tokenizer, text: &str) -> Result<Vec<u32>, PromptError> {
    tok.encode_fast(text, false)
        .map(|enc| enc.get_ids().to_vec())
        .map_err(|err| PromptError::Tokenizer(err.to_string()))
}

/// The [`MAX_OPTIONS`] option labels of a tokenizer (`decider.prompt.label_table`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelTable {
    /// `A`..`Z`, then two-letter labels in order, keeping those that encode to one token.
    pub names: Vec<String>,
    /// The single token id of each label.
    pub ids: Vec<u32>,
    /// Ids of `"\n("`, which opens each wide option.
    pub open: Vec<u32>,
}

impl LabelTable {
    pub fn new(tok: &Tokenizer) -> Result<LabelTable, PromptError> {
        let letters: Vec<char> = ('A'..='Z').collect();
        let singles = letters.iter().map(|c| c.to_string());
        let pairs = letters
            .iter()
            .flat_map(|a| letters.iter().map(move |b| format!("{a}{b}")));
        let (mut names, mut ids) = (Vec::new(), Vec::new());
        for name in singles.chain(pairs) {
            if let [id] = encode(tok, &name)?[..] {
                names.push(name);
                ids.push(id);
                if names.len() == MAX_OPTIONS {
                    break;
                }
            }
        }
        let mut distinct = ids.clone();
        distinct.sort_unstable();
        distinct.dedup();
        if names.len() != MAX_OPTIONS || distinct.len() != MAX_OPTIONS {
            return Err(PromptError::Labels {
                found: distinct.len(),
            });
        }
        Ok(LabelTable {
            names,
            ids,
            open: encode(tok, "\n(")?,
        })
    }
}

/// One prompt row: its ids, the answer slot of each question and each question's option count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub ids: Vec<u32>,
    pub slots: Vec<usize>,
    pub nopts: Vec<usize>,
}

/// One question of a row: its text and its option texts in label order.
pub type RowQuestion<'a> = (&'a str, &'a [String]);

fn numbered(word: &str, k: usize, multi: bool) -> String {
    if multi {
        format!("{word} {}", k + 1)
    } else {
        word.to_string()
    }
}

/// Ids of the option block of one question (`decider.prompt._options_ids`): narrow as one string, wide from ids.
pub fn options_ids(
    tok: &Tokenizer,
    labels: &LabelTable,
    options: &[String],
) -> Result<Vec<u32>, PromptError> {
    if options.len() <= NARROW {
        let text: String = options
            .iter()
            .zip(LETTERS.chars())
            .map(|(opt, letter)| format!("\n({letter}) {opt}"))
            .collect();
        return encode(tok, &text);
    }
    let mut out = Vec::new();
    for (opt, label) in options.iter().zip(&labels.ids) {
        out.extend_from_slice(&labels.open);
        out.push(*label);
        out.extend(encode(tok, &format!(") {opt}"))?);
    }
    Ok(out)
}

/// Ids of one plain `\n\nQuestion..: <text>\nOptions:...\nAnswer..: (` block (`prompt_fast.question_piece`).
pub fn question_piece(
    tok: &Tokenizer,
    labels: &LabelTable,
    text: &str,
    options: &[String],
    k: usize,
    multi: bool,
) -> Result<Vec<u32>, PromptError> {
    let head = format!("\n\n{}: {text}\nOptions:", numbered("Question", k, multi));
    let tail = format!("\n{}: (", numbered("Answer", k, multi));
    if options.len() <= NARROW {
        let opts: String = options
            .iter()
            .zip(LETTERS.chars())
            .map(|(opt, letter)| format!("\n({letter}) {opt}"))
            .collect();
        return encode(tok, &format!("{head}{opts}{tail}"));
    }
    let mut piece = encode(tok, &head)?;
    piece.extend(options_ids(tok, labels, options)?);
    piece.extend(encode(tok, &tail)?);
    Ok(piece)
}

/// Ids of chat answer piece `k`: `Answer: (`, or `Answer k: (` with several questions, later pieces after a newline.
pub fn chat_answer_ids(tok: &Tokenizer, k: usize, multi: bool) -> Result<Vec<u32>, PromptError> {
    let sep = if k == 0 { "" } else { "\n" };
    encode(tok, &format!("{sep}{}: (", numbered(ANSWER, k, multi)))
}

/// Ids of `"Context:\n" + context`, cut to `max_ctx_tokens` when given (`prompt_fast.context_ids`). Ardana serves
/// without a cap; the cap exists to reproduce decider's truncating builders.
pub fn context_ids(
    tok: &Tokenizer,
    context: &str,
    max_ctx_tokens: Option<usize>,
) -> Result<Vec<u32>, PromptError> {
    let mut ids = encode(tok, &format!("Context:\n{context}"))?;
    if let Some(cap) = max_ctx_tokens {
        ids.truncate(cap);
    }
    Ok(ids)
}

/// The rows of one request (`prompt_fast.build_rows`): the context is encoded once and shared, each row appends its
/// questions. Returns the items and the length of the shared prefix (template head plus context).
pub fn build_rows(
    tok: &Tokenizer,
    labels: &LabelTable,
    layout: &Layout,
    context: &str,
    rows: &[Vec<RowQuestion<'_>>],
    max_ctx_tokens: Option<usize>,
) -> Result<(Vec<Item>, usize), PromptError> {
    let mut ctx = match layout {
        Layout::Plain => Vec::new(),
        Layout::Chat { head, .. } => head.clone(),
    };
    ctx.extend(context_ids(tok, context, max_ctx_tokens)?);
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let multi = row.len() > 1;
        let mut ids = ctx.clone();
        let mut slots = Vec::with_capacity(row.len());
        let mut nopts = Vec::with_capacity(row.len());
        for (k, (text, options)) in row.iter().enumerate() {
            if !(2..=MAX_OPTIONS).contains(&options.len()) {
                return Err(PromptError::Options);
            }
            nopts.push(options.len());
            match layout {
                Layout::Plain => {
                    ids.extend(question_piece(tok, labels, text, options, k, multi)?);
                    slots.push(ids.len() - 1);
                }
                Layout::Chat { .. } => {
                    let head = format!("\n\n{}: {text}\nOptions:", numbered("Question", k, multi));
                    ids.extend(encode(tok, &head)?);
                    ids.extend(options_ids(tok, labels, options)?);
                }
            }
        }
        if let Layout::Chat { tail, .. } = layout {
            ids.extend_from_slice(tail);
            for k in 0..row.len() {
                ids.extend(chat_answer_ids(tok, k, multi)?);
                slots.push(ids.len() - 1);
            }
        }
        items.push(Item { ids, slots, nopts });
    }
    let ctx_len = ctx.len();
    Ok((items, ctx_len))
}

/// Input tokens of a request whose rows share the first `ctx_len` ids (`prompt_fast.unique_tokens`): the shared prefix
/// and the common prefix of the suffixes count once. No rows count 0.
pub fn unique_tokens(items: &[Item], ctx_len: usize) -> usize {
    let suffixes: Vec<&[u32]> = items.iter().map(|it| &it.ids[ctx_len..]).collect();
    match suffixes.as_slice() {
        [] => 0,
        [one] => ctx_len + one.len(),
        [first, rest @ ..] => {
            let short = suffixes.iter().map(|s| s.len()).min().unwrap_or(0);
            let lcp = (0..short)
                .take_while(|&i| rest.iter().all(|s| s[i] == first[i]))
                .count();
            ctx_len + lcp + suffixes.iter().map(|s| s.len() - lcp).sum::<usize>()
        }
    }
}

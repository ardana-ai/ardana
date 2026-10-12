//! THX-01's question sequences, token for token as the `thx01` 1.0.0 package builds them (`thx01/common.py`
//! `build_sequence` and `render_options`, `thx01/agent.py` `Agent._head_budget` and `Agent.system_one`), one per
//! question:
//!
//! ```text
//! [CLS] <type> question: <instructions> [SEP] [MASK] <option> [MASK] <option> … [SEP] <state> [SEP]
//! ```
//!
//! The model reads one logit at every `[MASK]`, the sequence's markers. Each option keeps [`OPTION_TOKENS`] tokens, the
//! head (instructions and options) `head_max_len` of the [`DecisionFormat`], raised to what the options need up to
//! half the window, and the instructions what the options leave (at least [`MIN_INSTRUCTIONS`]). The package then
//! cuts the state to the room left in the window; Ardana builds the whole sequence and the engine refuses one longer
//! than the window (Q9), so inside the window the two agree token for token.

use serde_json::Value;
use tokenizers::Tokenizer;

use crate::engine::DecisionSequence;
use crate::profile::DecisionFormat;
use crate::prompt::{self, PromptError};
use crate::py;
use crate::systemone::{Names, RenderedQuestion};

/// The files of a THX-01 checkpoint beside its `config.json` and `model.safetensors`, where the package reads them: its
/// decision configuration (limits and temperatures), the encoder's `config.json` and the tokenizer with its
/// configuration.
pub const DECISION_CONFIG: &str = "rl_agent_config.json";
pub const ENCODER_CONFIG: &str = "encoder/config.json";
pub const TOKENIZER: &str = "tokenizer/tokenizer.json";
pub const TOKENIZER_CONFIG: &str = "tokenizer/tokenizer_config.json";

/// The special tokens the package reads from the checkpoint's `tokenizer/tokenizer_config.json` (`cls_token`,
/// `sep_token`, `mask_token`): THX-01's Gemma tokenizer names them so.
pub const CLS_TOKEN: &str = "<bos>";
pub const SEP_TOKEN: &str = "<eos>";
pub const MASK_TOKEN: &str = "<mask>";
/// The tokens one option keeps after its marker.
pub const OPTION_TOKENS: usize = 48;
/// The tokens the instructions keep at least, however little the options leave.
pub const MIN_INSTRUCTIONS: usize = 8;
/// The room the options must leave the instructions before they are cut shorter, and what each keeps at least then.
const MIN_OPTION_ROOM: i64 = 16;
const MIN_PER_OPTION: i64 = 4;
/// `_head_budget`'s allowance for the type, the instructions and the separators beside the options.
const HEAD_ALLOWANCE: usize = 24;
/// The noul option texts of a question that describes neither side.
const NOUL_FALSE: &str = "no, the statement does not hold";
const NOUL_TRUE: &str = "yes, the statement holds";

/// The ids of the three special tokens in a tokenizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Specials {
    pub cls: u32,
    pub sep: u32,
    pub mask: u32,
}

impl Specials {
    /// The ids of [`CLS_TOKEN`], [`SEP_TOKEN`] and [`MASK_TOKEN`] in `tok`; an error naming the first one it lacks.
    pub fn of(tok: &Tokenizer) -> Result<Specials, PromptError> {
        let id = |token: &str| {
            tok.token_to_id(token).ok_or_else(|| {
                PromptError::Tokenizer(format!(
                    "the tokenizer has no {token} token, which a decision model's sequences need"
                ))
            })
        };
        Ok(Specials {
            cls: id(CLS_TOKEN)?,
            sep: id(SEP_TOKEN)?,
            mask: id(MASK_TOKEN)?,
        })
    }
}

/// `serialize_state`: a string as itself, any other state as `json.dumps(state, ensure_ascii=False)`; the package
/// indexes no arrays, unlike decider's `render_state`.
pub fn serialize_state(state: &Value) -> String {
    match state {
        Value::String(s) => s.clone(),
        other => py::dumps(other, false),
    }
}

/// `render_options`, from the question as `systemone::render_question` rendered it: a choice option is its name, or
/// `name: description`, as decider renders it too; a score level is `level <i>: <text>`; a noul question's two are
/// `false: …` and `true: …`, with the question's descriptions or the package's defaults.
pub fn options(rq: &RenderedQuestion) -> Vec<String> {
    match &rq.names {
        Names::Choice(_) => rq.options.clone(),
        Names::Score(_) => rq
            .legend
            .iter()
            .enumerate()
            .map(|(i, text)| format!("level {i}: {text}"))
            .collect(),
        Names::Noul => [("false", "no", NOUL_FALSE), ("true", "yes", NOUL_TRUE)]
            .into_iter()
            .zip(&rq.options)
            .map(|((word, rendered, default), option)| {
                // decider's option is the bare word, or the word, `: ` and the description as it is.
                let description = option
                    .strip_prefix(rendered)
                    .and_then(|rest| rest.strip_prefix(": "))
                    .unwrap_or(default);
                format!("{word}: {description}")
            })
            .collect(),
    }
}

/// `Agent._head_budget`: `head_max_len`, raised to what the options need (each its tokens plus its marker, at most
/// [`OPTION_TOKENS`] + 1, plus [`HEAD_ALLOWANCE`]) and capped at half the window. The package counts the options'
/// tokens here before it blanks `<mask>` in them, as [`build_sequence`] does after.
fn head_budget(
    tok: &Tokenizer,
    format: &DecisionFormat,
    options: &[String],
) -> Result<usize, PromptError> {
    let mut need = HEAD_ALLOWANCE;
    for option in options {
        need += (prompt::encode(tok, &format!(" {option}"))?.len() + 1).min(OPTION_TOKENS + 1);
    }
    Ok(format.head_max_len.max(need).min(format.window / 2))
}

/// `build_sequence` without its cut of the state: the question `rq` over `state` (already serialised), whole. `<mask>`
/// in the instructions, an option or the state is replaced by a space, as the package does, so the markers alone are
/// mask tokens.
pub fn build_sequence(
    tok: &Tokenizer,
    specials: Specials,
    format: &DecisionFormat,
    state: &str,
    rq: &RenderedQuestion,
) -> Result<DecisionSequence, PromptError> {
    let blank = |text: &str| text.replace(MASK_TOKEN, " ");
    let options = options(rq);
    let head_max_len = head_budget(tok, format, &options)? as i64;
    let mut head = prompt::encode(
        tok,
        &format!("{} question: {}", rq.kind, blank(&rq.question)),
    )?;
    let mut option_ids = Vec::with_capacity(options.len());
    for option in &options {
        let mut ids = vec![specials.mask];
        let encoded = prompt::encode(tok, &format!(" {}", blank(option)))?;
        ids.extend(encoded.into_iter().take(OPTION_TOKENS));
        option_ids.push(ids);
    }
    let room = |option_ids: &[Vec<u32>]| {
        head_max_len - option_ids.iter().map(|ids| ids.len() as i64).sum::<i64>()
    };
    let mut option_room = room(&option_ids);
    if option_room < MIN_OPTION_ROOM {
        let per = (head_max_len - MIN_OPTION_ROOM)
            .div_euclid(option_ids.len().max(1) as i64)
            .max(MIN_PER_OPTION);
        for ids in &mut option_ids {
            ids.truncate(per as usize);
        }
        option_room = room(&option_ids);
    }
    head.truncate(option_room.max(MIN_INSTRUCTIONS as i64) as usize);

    let mut ids = vec![specials.cls];
    ids.extend(head);
    ids.push(specials.sep);
    let mut markers = Vec::with_capacity(option_ids.len());
    for option in option_ids {
        markers.push(ids.len());
        ids.extend(option);
    }
    ids.push(specials.sep);
    ids.extend(prompt::encode(tok, &blank(state))?);
    ids.push(specials.sep);
    Ok(DecisionSequence {
        ids,
        markers,
        kind: rq.kind,
    })
}

/// A word-level tokenizer for the tests of this crate: every whitespace-separated word of `words` is one token,
/// anything else `<unk>`, with the special tokens `specials` added.
#[cfg(test)]
pub(crate) fn word_tokenizer(words: &[&str], specials: &[&str]) -> Tokenizer {
    use tokenizers::AddedToken;
    use tokenizers::models::wordlevel::WordLevel;
    use tokenizers::pre_tokenizers::whitespace::WhitespaceSplit;

    let vocab = ["<pad>", "<unk>"]
        .into_iter()
        .chain(words.iter().copied())
        .enumerate()
        .map(|(id, word)| (word.to_string(), id as u32))
        .collect();
    let model = WordLevel::builder()
        .vocab(vocab)
        .unk_token("<unk>".into())
        .build()
        .expect("a word-level vocabulary builds");
    let mut tok = Tokenizer::new(model);
    tok.with_pre_tokenizer(Some(WhitespaceSplit));
    let _ = tok.add_special_tokens(specials.iter().map(|token| AddedToken::from(*token, true)));
    tok
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::profile::AnswerType;
    use crate::systemone::render_question;

    /// [`word_tokenizer`] with THX-01's three special tokens.
    fn tokenizer(words: &[&str]) -> Tokenizer {
        word_tokenizer(words, &[CLS_TOKEN, SEP_TOKEN, MASK_TOKEN])
    }

    fn format(window: usize, head_max_len: usize) -> DecisionFormat {
        DecisionFormat {
            window,
            max_options: 24,
            head_max_len,
        }
    }

    /// The package's layout: `[CLS] <type> question: <instructions> [SEP]`, a `[MASK]` before each option's tokens,
    /// `[SEP]`, the state's tokens, `[SEP]`; the markers are the mask positions, the options are rendered as the
    /// package renders each type, a JSON state is serialised without index annotation, and `<mask>` in a text is
    /// blanked.
    #[test]
    fn sequences_follow_the_package_layout() {
        let tok = tokenizer(&[
            "choice",
            "question:",
            "Which",
            "team?",
            "billing:",
            "charges",
            "sales",
            "the",
            "card",
            "level",
            "0:",
            "calm",
            "1:",
            "upset",
            "false:",
            "no,",
            "statement",
            "does",
            "not",
            "hold",
            "true:",
            "yes,",
            "holds",
            "Refund?",
            "noul",
            "score",
            "How",
            "upset?",
            "{\"a\":",
            "[1,",
            "2]}",
        ]);
        let specials = Specials::of(&tok).unwrap();
        let id = |word: &str| tok.token_to_id(word).unwrap();
        let choice = render_question(&json!({"type": "choice", "instructions": "Which team?",
            "criteria": {"billing": "charges", "sales": null}}))
        .unwrap();
        let seq = build_sequence(&tok, specials, &format(1024, 256), "the card", &choice).unwrap();
        let want = [
            specials.cls,
            id("choice"),
            id("question:"),
            id("Which"),
            id("team?"),
            specials.sep,
            specials.mask,
            id("billing:"),
            id("charges"),
            specials.mask,
            id("sales"),
            specials.sep,
            id("the"),
            id("card"),
            specials.sep,
        ];
        assert_eq!(seq.ids, want);
        assert_eq!(seq.markers, [6, 9]);
        assert_eq!(seq.kind, AnswerType::Choice);

        let score = render_question(&json!({"type": "score", "instructions": "How upset?",
            "criteria": ["calm", "upset"]}))
        .unwrap();
        assert_eq!(options(&score), ["level 0: calm", "level 1: upset"]);
        let seq = build_sequence(&tok, specials, &format(1024, 256), "the card", &score).unwrap();
        assert_eq!(seq.kind, AnswerType::Score);
        assert_eq!(&seq.ids[1..3], &[id("score"), id("question:")]);
        assert_eq!(seq.markers, [6, 10]);
        assert_eq!(
            &seq.ids[6..10],
            &[specials.mask, id("level"), id("0:"), id("calm")]
        );

        let noul = render_question(&json!({"type": "noul", "instructions": "Refund?"})).unwrap();
        assert_eq!(
            options(&noul),
            [
                "false: no, the statement does not hold",
                "true: yes, the statement holds"
            ]
        );
        let described = render_question(&json!({"type": "noul", "instructions": "Refund?",
            "criteria": {"true": "charges", "false": {"a": [1, 2]}}}))
        .unwrap();
        assert_eq!(
            options(&described),
            ["false: {\"a\": [1, 2]}", "true: charges"]
        );
        let seq = build_sequence(&tok, specials, &format(1024, 256), "the card", &noul).unwrap();
        assert_eq!(seq.kind, AnswerType::Noul);
        assert_eq!(seq.ids[1], id("noul"));
        assert_eq!(seq.markers.len(), 2);
        assert_eq!(seq.ids[seq.markers[0]], specials.mask);
        assert_eq!(
            &seq.ids[seq.markers[0] + 1..seq.markers[0] + 3],
            &[id("false:"), id("no,")]
        );

        assert_eq!(serialize_state(&json!("text")), "text");
        assert_eq!(serialize_state(&json!({"a": [1, 2]})), r#"{"a": [1, 2]}"#);
        let masked = build_sequence(
            &tok,
            specials,
            &format(1024, 256),
            "the <mask> card",
            &choice,
        )
        .unwrap();
        assert_eq!(masked.ids, want);
        let masked_question = render_question(
            &json!({"type": "choice", "instructions": "Which <mask> team?",
            "criteria": {"billing": "charges", "sales": null}}),
        )
        .unwrap();
        let masked = build_sequence(
            &tok,
            specials,
            &format(1024, 256),
            "the card",
            &masked_question,
        )
        .unwrap();
        assert_eq!(masked.ids, want);
    }

    /// The package's cuts: an option keeps 48 tokens after its marker; the head budget is `head_max_len`, raised to
    /// what the options need (their tokens plus one each, at most 49, plus 24) and capped at half the window; when the
    /// options leave the instructions under 16 tokens each option is cut to its share of the budget less 16 (at least
    /// 4), and the instructions keep what is left, at least 8 tokens. The state is never cut: the engine refuses a
    /// sequence past the window.
    #[test]
    fn sequences_cut_as_the_package_cuts() {
        let words: Vec<String> = (0..80).map(|i| format!("w{i}")).collect();
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let tok = tokenizer(&words);
        let specials = Specials::of(&tok).unwrap();
        let text = |n: usize| -> String { words[..n].join(" ") };
        let long_options = render_question(&json!({"type": "choice", "instructions": text(30),
            "criteria": {text(60): null, text(20): null}}))
        .unwrap();
        // The second option is "w0 … w19" and no `name: description`: 20 tokens after its marker.
        let seq =
            build_sequence(&tok, specials, &format(1024, 256), &text(5), &long_options).unwrap();
        assert_eq!(seq.markers[1] - seq.markers[0], 1 + OPTION_TOKENS);
        // The instructions keep "choice question:" (unknown words, two tokens) and their 30: the options need 49 + 21
        // + 24 = 94 under the 256 budget, which leaves them 256 - 70 = 186.
        assert_eq!(seq.markers[0], 1 + 32 + 1);
        assert_eq!(seq.ids.len(), 1 + 32 + 1 + 49 + 21 + 1 + 5 + 1);

        // A budget the options exceed by themselves: 40 is raised to what they need (94), the options stay whole and
        // the instructions keep 94 - 70 = 24 tokens.
        let seq =
            build_sequence(&tok, specials, &format(1024, 40), &text(5), &long_options).unwrap();
        assert_eq!(seq.markers[0], 1 + 24 + 1);
        assert_eq!(seq.markers[1] - seq.markers[0], 49);
        // Capped at half the window: a 120-token window gives 60; the options' 70 leave less than 16, so each is cut
        // to (60 - 16) / 2 = 22 tokens, marker included (the second has 21 and stays), and the instructions keep
        // 60 - 43 = 17.
        let seq =
            build_sequence(&tok, specials, &format(120, 40), &text(5), &long_options).unwrap();
        assert_eq!(seq.markers[0], 1 + 17 + 1);
        assert_eq!(seq.markers[1] - seq.markers[0], 22);
        assert_eq!(seq.ids[seq.markers[1] + 21], specials.sep);
        // Options that leave the budget nothing even cut to 4 each: the instructions still keep 8.
        let many: serde_json::Map<String, Value> = (0..20)
            .map(|i| (format!("{} w{}", text(10), 60 + i), Value::Null))
            .collect();
        let crowded =
            render_question(&json!({"type": "choice", "instructions": text(30), "criteria": many}))
                .unwrap();
        let seq = build_sequence(&tok, specials, &format(64, 40), &text(3), &crowded).unwrap();
        assert_eq!(seq.markers[0], 1 + 8 + 1);
        assert_eq!(seq.markers[1] - seq.markers[0], 4);

        // The state goes in whole, however long.
        let seq = build_sequence(&tok, specials, &format(16, 8), &text(70), &long_options).unwrap();
        assert_eq!(seq.ids.len(), seq.markers[1] + 4 + 1 + 70 + 1);
        assert_eq!(seq.kind, AnswerType::Choice);
    }

    /// A tokenizer without the three special tokens cannot build sequences, and says which it lacks.
    #[test]
    fn specials_name_the_missing_token() {
        let tok = word_tokenizer(&["a"], &[CLS_TOKEN, SEP_TOKEN]);
        let err = Specials::of(&tok).unwrap_err();
        assert!(err.to_string().contains("<mask>"), "{err}");
        assert!(Specials::of(&tokenizer(&["a"])).is_ok());
    }
}

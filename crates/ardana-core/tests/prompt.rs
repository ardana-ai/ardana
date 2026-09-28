//! R2.1 and R2.2: the label table and the prompt rows of decider 1.6.0, token for token, on decider-2b's tokenizer.
//!
//! `tests/data/decider/layout_cases.json` holds the inputs of every `prompt:` and `serve:` case of decider's
//! `tests/layout_cases.py` (exported by `cargo xtask export-decider`); the rows built here must hash to decider's
//! `plain_layout_pins.json`. decider's schema-first layout (its schema cache, which Ardana does not serve) is composed
//! here from the same `ardana_core::prompt` pieces so its pins check those pieces too.

mod common;

use anyhow::{Context, Result, bail, ensure};
use ardana_core::Layout;
use ardana_core::prompt::{self, Item, LabelTable, RowQuestion};
use ardana_core::systemone;
use indexmap::IndexMap;
use serde_json::{Value, json};
use tokenizers::Tokenizer;

#[test]
fn label_table_qwen35() -> Result<()> {
    let tok = common::decider_2b_tokenizer()?;
    let labels = LabelTable::new(&tok)?;
    assert_eq!(labels.names.len(), 255);
    assert_eq!(labels.ids.len(), 255);
    let letters: Vec<String> = ('A'..='Z').map(String::from).collect();
    assert_eq!(labels.names[..26], letters[..]);
    assert_eq!(labels.ids[..26], (32..=57).collect::<Vec<u32>>()[..]);
    assert!(labels.names[26..].iter().all(|n| n.len() == 2));
    assert_eq!(labels.names.last().map(String::as_str), Some("JT"));
    Ok(())
}

/// The decider item dict (`layout_cases._item`) as JSON.
fn item_json(item: &Item, golds: &[i64], perms: &[Vec<usize>], prefix_len: Option<usize>) -> Value {
    let mut v = json!({
        "ids": item.ids, "slots": item.slots, "golds": golds, "nopts": item.nopts, "perms": perms,
    });
    if let Some(p) = prefix_len {
        v["prefix_len"] = json!(p);
    }
    v
}

fn strings(v: &Value) -> Result<Vec<String>> {
    v.as_array()
        .context("expected a list of strings")?
        .iter()
        .map(|s| s.as_str().map(String::from).context("expected a string"))
        .collect()
}

fn usizes(v: &Value) -> Result<Vec<usize>> {
    v.as_array()
        .context("expected a list of integers")?
        .iter()
        .map(|n| {
            n.as_u64()
                .map(|n| n as usize)
                .context("expected an integer")
        })
        .collect()
}

fn num(v: &Value, key: &str) -> Result<usize> {
    v[key]
        .as_u64()
        .map(|n| n as usize)
        .with_context(|| format!("`{key}` is not an integer"))
}

/// `[[text, [options], (gold)], ...]` -> texts, options, golds.
fn questions(v: &Value) -> Result<Vec<(String, Vec<String>, i64)>> {
    v.as_array()
        .context("expected questions")?
        .iter()
        .map(|q| {
            let text = q[0].as_str().context("question text")?.to_string();
            Ok((
                text,
                strings(&q[1])?,
                q.get(2).and_then(Value::as_i64).unwrap_or(0),
            ))
        })
        .collect()
}

fn identity(n: usize) -> Vec<usize> {
    (0..n).collect()
}

/// decider's `schema_prefix_ids`: every question block, headers without a leading newline on the first.
fn schema_prefix(
    tok: &Tokenizer,
    labels: &LabelTable,
    qs: &[(String, Vec<String>)],
) -> Result<Vec<u32>> {
    let multi = qs.len() > 1;
    let mut ids = Vec::new();
    for (k, (text, options)) in qs.iter().enumerate() {
        let sep = if k > 0 { "\n\n" } else { "" };
        let num = if multi {
            format!(" {}", k + 1)
        } else {
            String::new()
        };
        ids.extend(prompt::encode(
            tok,
            &format!("{sep}Question{num}: {text}\nOptions:"),
        )?);
        ids.extend(prompt::options_ids(tok, labels, options)?);
    }
    Ok(ids)
}

/// decider's `schema_suffix_ids`: the context after the questions, then one answer slot per question.
fn schema_suffix(
    tok: &Tokenizer,
    context: &str,
    n_q: usize,
    cap: usize,
) -> Result<(Vec<u32>, Vec<usize>)> {
    let mut ids = prompt::encode(tok, "\n\nContext:\n")?;
    let mut ctx = prompt::encode(tok, context)?;
    ctx.truncate(cap);
    ids.extend(ctx);
    let mut slots = Vec::new();
    for k in 0..n_q {
        let sep = if k == 0 { "\n\n" } else { "\n" };
        let num = if n_q > 1 {
            format!(" {}", k + 1)
        } else {
            String::new()
        };
        ids.extend(prompt::encode(tok, &format!("{sep}Answer{num}: ("))?);
        slots.push(ids.len() - 1);
    }
    Ok((ids, slots))
}

/// One plain row holding `qs` (decider's `prompt.build` with the options already in their final order).
fn packed_row(
    tok: &Tokenizer,
    labels: &LabelTable,
    context: &str,
    qs: &[(String, Vec<String>)],
    cap: usize,
) -> Result<Item> {
    let row: Vec<RowQuestion<'_>> = qs.iter().map(|(t, o)| (t.as_str(), o.as_slice())).collect();
    let (mut items, _) =
        prompt::build_rows(tok, labels, &Layout::Plain, context, &[row], Some(cap))?;
    Ok(items.remove(0))
}

/// The value decider pinned for one exported case.
fn case_value(tok: &Tokenizer, labels: &LabelTable, data: &Value, spec: &Value) -> Result<Value> {
    let text = |key: &str| -> Result<&str> {
        let i = num(spec, key)?;
        data["texts"][i].as_str().context("texts entry")
    };
    match spec["kind"].as_str().context("case kind")? {
        "build" => {
            let context = text("context")?;
            let cap = num(spec, "cap")?;
            let qs = questions(&spec["questions"])?;
            let perms: Vec<Vec<usize>> = spec["perms"]
                .as_array()
                .context("perms")?
                .iter()
                .map(usizes)
                .collect::<Result<_>>()?;
            let golds: Vec<i64> = qs
                .iter()
                .zip(&perms)
                .map(|((_, _, gold), perm)| {
                    perm.iter()
                        .position(|&i| i as i64 == *gold)
                        .map_or(-1, |p| p as i64)
                })
                .collect();
            let ordered: Vec<(String, Vec<String>)> = qs
                .iter()
                .zip(&perms)
                .map(|((t, opts, _), perm)| {
                    (t.clone(), perm.iter().map(|&i| opts[i].clone()).collect())
                })
                .collect();
            match spec["layout"].as_str() {
                Some("state_first") => {
                    let item = packed_row(tok, labels, context, &ordered, cap)?;
                    Ok(item_json(&item, &golds, &perms, None))
                }
                Some("schema_first") => {
                    let pre = schema_prefix(tok, labels, &ordered)?;
                    let (suf, slots) = schema_suffix(tok, context, ordered.len(), cap)?;
                    let item = Item {
                        ids: [pre.clone(), suf].concat(),
                        slots: slots.iter().map(|s| pre.len() + s).collect(),
                        nopts: perms.iter().map(Vec::len).collect(),
                    };
                    Ok(item_json(&item, &golds, &perms, Some(pre.len())))
                }
                other => bail!("unknown layout {other:?}"),
            }
        }
        "schema_prefix" => {
            let qs: Vec<(String, Vec<String>)> = questions(&spec["questions"])?
                .into_iter()
                .map(|(t, o, _)| (t, o))
                .collect();
            Ok(json!(schema_prefix(tok, labels, &qs)?))
        }
        "schema_suffix" => {
            let (ids, slots) =
                schema_suffix(tok, text("context")?, num(spec, "n_q")?, num(spec, "cap")?)?;
            Ok(json!([ids, slots]))
        }
        "build_rows" => {
            let rows: Vec<Vec<(String, Vec<String>)>> = spec["rows"]
                .as_array()
                .context("rows")?
                .iter()
                .map(|row| {
                    Ok(questions(row)?
                        .into_iter()
                        .map(|(t, o, _)| (t, o))
                        .collect())
                })
                .collect::<Result<_>>()?;
            let refs: Vec<Vec<RowQuestion<'_>>> = rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|(t, o)| (t.as_str(), o.as_slice()))
                        .collect()
                })
                .collect();
            let (items, ctx_len) = prompt::build_rows(
                tok,
                labels,
                &Layout::Plain,
                text("context")?,
                &refs,
                Some(num(spec, "cap")?),
            )?;
            let items: Vec<Value> = items.iter().map(rows_item_json).collect();
            Ok(json!([items, ctx_len]))
        }
        "prepare" => {
            let state = &data["states"][num(spec, "state")?];
            let independent = spec["independent"].as_bool().context("independent")?;
            let isolated = spec["isolated"].as_bool().context("isolated")?;
            let mut rqs = IndexMap::new();
            for (id, q) in data["questions"].as_object().context("questions")? {
                rqs.insert(
                    id.clone(),
                    systemone::render_question(q).map_err(anyhow::Error::msg)?,
                );
            }
            let (rows, index) = systemone::plan_rows(&rqs, isolated && independent);
            let pairs: Vec<RowQuestion<'_>> = rows
                .iter()
                .map(|r| (r.question.as_str(), r.options.as_slice()))
                .collect();
            let grouped: Vec<Vec<RowQuestion<'_>>> = if independent {
                pairs.iter().map(|p| vec![*p]).collect()
            } else {
                vec![pairs]
            };
            let context = systemone::render_state(state);
            let (items, ctx_len) = prompt::build_rows(
                tok,
                labels,
                &Layout::Plain,
                &context,
                &grouped,
                Some(num(spec, "cap")?),
            )?;
            let index: Vec<Value> = index
                .iter()
                .map(|s| json!([s.id, s.kind.as_str(), s.first, s.count]))
                .collect();
            let items: Vec<Value> = items.iter().map(rows_item_json).collect();
            Ok(json!([index, items, ctx_len]))
        }
        "decide" => {
            let qs: Vec<(String, Vec<String>)> = questions(&spec["questions"])?
                .into_iter()
                .map(|(t, o, _)| (t, o))
                .collect();
            let item = packed_row(tok, labels, text("context")?, &qs, num(spec, "cap")?)?;
            let perms: Vec<Vec<usize>> = qs.iter().map(|(_, o)| identity(o.len())).collect();
            let options: Vec<&Vec<String>> = qs.iter().map(|(_, o)| o).collect();
            Ok(json!([
                options,
                item_json(&item, &vec![0; qs.len()], &perms, None)
            ]))
        }
        other => bail!("unknown case kind {other}"),
    }
}

/// A `build_rows` item: golds are 0 and options keep their order.
fn rows_item_json(item: &Item) -> Value {
    let perms: Vec<Vec<usize>> = item.nopts.iter().map(|&n| identity(n)).collect();
    item_json(item, &vec![0; item.nopts.len()], &perms, None)
}

#[test]
fn decider_pins() -> Result<()> {
    let tok = common::decider_2b_tokenizer()?;
    let labels = LabelTable::new(&tok)?;
    let data = common::decider_data("layout_cases.json")?;
    let pins = common::decider_data("plain_layout_pins.json")?;
    let pins = pins["pins"].as_object().context("pins")?;
    let cases = data["cases"].as_object().context("cases")?;
    let (mut prompt_n, mut serve_n, mut bad) = (0, 0, Vec::new());
    for (key, spec) in cases {
        let value = case_value(&tok, &labels, &data, spec).with_context(|| key.clone())?;
        let pin = pins
            .get(key)
            .and_then(Value::as_str)
            .with_context(|| format!("no pin {key}"))?;
        if common::digest(&value) != pin {
            bad.push(key.clone());
        }
        if key.starts_with("prompt:") {
            prompt_n += 1;
        } else if key.starts_with("serve:") {
            serve_n += 1;
        }
    }
    ensure!(
        bad.is_empty(),
        "{} of {} cases differ from their pins, e.g. {:?}",
        bad.len(),
        cases.len(),
        &bad[..bad.len().min(5)]
    );
    assert_eq!((prompt_n, serve_n), (149, 74));
    Ok(())
}

const QWEN_HEAD: &str = "<|im_start|>user\n";
const QWEN_TAIL: &str = "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n";

fn qwen_chat(tok: &Tokenizer) -> Result<Layout> {
    Ok(Layout::Chat {
        head: prompt::encode(tok, QWEN_HEAD)?,
        tail: prompt::encode(tok, QWEN_TAIL)?,
    })
}

fn one_row(
    tok: &Tokenizer,
    labels: &LabelTable,
    layout: &Layout,
    context: &str,
    qs: &[(&str, &[&str])],
) -> Result<Item> {
    let owned: Vec<(&str, Vec<String>)> = qs
        .iter()
        .map(|(t, o)| (*t, o.iter().map(|s| s.to_string()).collect()))
        .collect();
    let row: Vec<RowQuestion<'_>> = owned.iter().map(|(t, o)| (*t, o.as_slice())).collect();
    let (mut items, _) = prompt::build_rows(tok, labels, layout, context, &[row], None)?;
    Ok(items.remove(0))
}

/// decider `tests/test_layout.py::test_state_first_chat_text` and
/// `test_plain_and_chat_differ_only_by_the_wrapping`, with the Qwen3.5 head and tail given as ids.
#[test]
fn chat_rows_match_decider_text() -> Result<()> {
    let tok = common::decider_2b_tokenizer()?;
    let labels = LabelTable::new(&tok)?;
    let chat = qwen_chat(&tok)?;
    let Layout::Chat { head, tail } = &chat else {
        unreachable!()
    };
    assert_eq!((head.len(), tail.len()), (3, 9));

    let item = one_row(
        &tok,
        &labels,
        &chat,
        "state",
        &[("Which?", &["a", "b"]), ("Urgent?", &["no", "yes"])],
    )?;
    let text = tok.decode(&item.ids, false).map_err(anyhow::Error::msg)?;
    let want = format!(
        "{QWEN_HEAD}Context:\nstate\n\nQuestion 1: Which?\nOptions:\n(A) a\n(B) b\n\nQuestion 2: Urgent?\nOptions:\n(A) no\n(B) yes{QWEN_TAIL}Answer 1: (\nAnswer 2: ("
    );
    assert_eq!(text, want);
    assert!(
        item.slots.iter().all(|&s| item.ids[s] == 318),
        "every slot is the \" (\" token"
    );

    let qs: &[(&str, &[&str])] = &[("Which team?", &["billing", "technical", "sales"])];
    let context = "My card was charged twice.";
    let plain = tok
        .decode(
            &one_row(&tok, &labels, &Layout::Plain, context, qs)?.ids,
            false,
        )
        .map_err(anyhow::Error::msg)?;
    let chat_text = tok
        .decode(&one_row(&tok, &labels, &chat, context, qs)?.ids, false)
        .map_err(anyhow::Error::msg)?;
    let body = plain
        .strip_suffix("\nAnswer: (")
        .context("plain row ends with its answer piece")?;
    assert_eq!(chat_text, format!("{QWEN_HEAD}{body}{QWEN_TAIL}Answer: ("));
    Ok(())
}

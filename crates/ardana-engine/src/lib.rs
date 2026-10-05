//! The in-tab engine's reader: `ardana-core`'s tokenizer, planner and readout as a WebAssembly module of its own. trunk
//! builds it beside the playground (`data-type="worker"` in `crates/ardana-playground/index.html`, served at
//! `/engine/`), and the page imports it only once an "In browser" row is picked, as it imports onnxruntime-web: a page
//! that runs server models alone never downloads it. The page keeps the rest of a run in the tab (the browser files,
//! the stages, Stop, onnxruntime-web); this module reads a browser variant's profile and tokenizer, plans a request as
//! the server does, hands out the rows to decode and answers with the server's own bytes, refusals included.
//!
//! Every object it hands out holds memory of this module's own, which the page frees (`free()`) once done with it.

use std::rc::Rc;

use ardana_api::SystemOneRequest;
use ardana_core::{
    DecideError, Decider, Decode, LoadOptions, LoadedModel, ModelProfile, Plan, Tokenizer,
};
use wasm_bindgen::prelude::*;

/// A browser variant's profile, as `GET /v1/browser/<name>/profile` sends it.
#[wasm_bindgen]
pub struct Profile(ModelProfile);

/// Reads the profile the server sent for a browser variant.
#[wasm_bindgen]
pub fn profile(json: &str) -> Result<Profile, String> {
    serde_json::from_str(json)
        .map(Profile)
        .map_err(|err| format!("the server sent an unreadable profile: {err}"))
}

/// A browser variant's reader: its tokenizer and profile, read as the server reads the same model.
#[wasm_bindgen]
pub struct Reader(Rc<Decider>);

/// The reader of the browser variant of `name`, from the bytes of its `tokenizer.json` and its profile.
#[wasm_bindgen]
pub fn reader(name: &str, tokenizer: &[u8], profile: &Profile) -> Result<Reader, String> {
    let tokenizer = Tokenizer::from_bytes(tokenizer)
        .map_err(|err| format!("the tokenizer of {name} does not load: {err}"))?;
    let decider = Decider::new(tokenizer, profile.0.clone())
        .map_err(|err| format!("the tokenizer of {name} does not fit the model: {err}"))?;
    Ok(Reader(Rc::new(decider)))
}

#[wasm_bindgen]
impl Reader {
    /// Plans `body`, a `/v1/systemone` request, as the server plans it before anything is decoded.
    pub fn plan(&self, body: &str) -> Result<Run, String> {
        let request: SystemOneRequest = serde_json::from_str(body)
            .map_err(|err| format!("the request is not a /v1/systemone body: {err}"))?;
        Ok(Run::new(self.0.clone(), &request))
    }
}

/// A request planned in the tab and decoded there row by row: the page decodes each row's ids on onnxruntime-web and
/// hands back the logits at the row's labels, and the run answers as the server answers the request.
#[wasm_bindgen]
pub struct Run {
    decider: Rc<Decider>,
    /// The plan; or why the request is answered without a decode: the API's refusal, or rows the browser graph cannot
    /// read.
    plan: Result<Plan, DecideError>,
    /// The logits of each row decoded so far, at its labels.
    rows: Vec<Vec<f32>>,
    /// Why a row did not decode.
    failed: Option<String>,
}

#[wasm_bindgen]
impl Run {
    /// Whether the request is answered without a decode: refused as the server refuses it (422, 413), or planned into
    /// rows the browser graph cannot read.
    pub fn refused(&self) -> bool {
        self.plan.is_err()
    }

    /// The rows to decode; none for a refused request.
    pub fn rows(&self) -> usize {
        self.plan.as_ref().map_or(0, Plan::rows)
    }

    /// The token ids of row `row`.
    pub fn ids(&self, row: usize) -> Vec<u32> {
        self.decode(row)
            .map(|decode| decode.ids.to_vec())
            .unwrap_or_default()
    }

    /// The label ids the logits of row `row` are read at.
    pub fn labels(&self, row: usize) -> Vec<u32> {
        self.decode(row)
            .map(|decode| decode.label_ids.to_vec())
            .unwrap_or_default()
    }

    /// The logits of the next row, at its labels.
    pub fn push(&mut self, logits: &[f32]) {
        self.rows.push(logits.to_vec());
    }

    /// A row did not decode, for `reason`: the run answers as the server answers a runtime that failed.
    pub fn fail(&mut self, reason: &str) {
        self.failed = Some(reason.to_string());
    }

    /// The status the API answers the request with: at once for a refused request, else once every row is decoded
    /// (a run with rows missing answers as a runtime that failed).
    pub fn status(&self) -> u16 {
        self.answer().0
    }

    /// The body text the API answers the request with, as the server serialises it.
    pub fn body(&self) -> String {
        self.answer().1
    }
}

impl Run {
    fn new(decider: Rc<Decider>, request: &SystemOneRequest) -> Run {
        let plan = decider.plan(request).and_then(|plan| {
            // The browser graph returns the logits of a row's last position only: every planned row must end in its
            // one slot.
            let unread = decider.decodes(&plan).find_map(|row| {
                (row.slots != [row.ids.len().saturating_sub(1)]).then(|| {
                    format!(
                        "a row reads slots {:?} of {} tokens; the browser graph reads only the last",
                        row.slots,
                        row.ids.len()
                    )
                })
            });
            match unread {
                Some(msg) => Err(DecideError::Runtime(anyhow::anyhow!(msg))),
                None => Ok(plan),
            }
        });
        Run {
            decider,
            plan,
            rows: Vec::new(),
            failed: None,
        }
    }

    fn decode(&self, row: usize) -> Option<Decode<'_>> {
        self.decider.decodes(self.plan.as_ref().ok()?).nth(row)
    }

    /// The status and body text the API answers the request with.
    fn answer(&self) -> (u16, String) {
        let answered = match (&self.plan, &self.failed) {
            (Err(err), _) => return refusal(err),
            (Ok(_), Some(reason)) => Err(DecideError::Runtime(anyhow::anyhow!(reason.clone()))),
            (Ok(plan), None) => {
                let rows: Vec<_> = self.rows.iter().map(|row| vec![row.clone()]).collect();
                self.decider.run(&mut Replay(rows.into_iter()), plan)
            }
        };
        match answered {
            Ok(response) => (200, body_text(&response)),
            Err(err) => refusal(&err),
        }
    }
}

/// How the API answers a request it does not decide: `DecideError`'s status and body, as the server sends them.
fn refusal(err: &DecideError) -> (u16, String) {
    (err.status(), body_text(&err.body()))
}

/// A body as the API serialises it (axum's `Json`: compact).
fn body_text(body: &impl serde::Serialize) -> String {
    serde_json::to_string(body).expect("API bodies serialise")
}

/// The logits the page decoded, handed back row by row to [`Decider::run`].
struct Replay(std::vec::IntoIter<Vec<Vec<f32>>>);

impl LoadedModel for Replay {
    /// The window of the server's default load, so a request too long for it is refused alike.
    fn n_ctx(&self) -> usize {
        LoadOptions::default().n_ctx as usize
    }

    fn slot_logits(&mut self, _: &[u32], _: &[usize], _: &[u32]) -> anyhow::Result<Vec<Vec<f32>>> {
        self.0
            .next()
            .ok_or_else(|| anyhow::anyhow!("fewer decoded rows than planned rows"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use anyhow::{Context, Result};
    use ardana_core::{Layout, from_decider_config};
    use serde_json::Value;

    use super::*;

    /// A file of the pinned decider-2b-GGUF snapshot that `cargo xtask fetch` puts into `tmp/hf` (cargo's `[env]`
    /// points `HF_HOME` there).
    fn decider_2b(file: &str) -> Result<Vec<u8>> {
        let repo = PathBuf::from(std::env::var_os("HF_HOME").context("HF_HOME is not set")?)
            .join("hub/models--Mapika--decider-2b-GGUF");
        let commit = std::fs::read_to_string(repo.join("refs/main")).with_context(|| {
            format!(
                "{} has no refs/main; run `cargo xtask fetch`",
                repo.display()
            )
        })?;
        let path = repo.join("snapshots").join(commit.trim()).join(file);
        std::fs::read(&path)
            .with_context(|| format!("{} is missing; run `cargo xtask fetch`", path.display()))
    }

    /// The profile JSON the server sends for decider:2b, and the bytes of its tokenizer.
    fn decider_2b_files() -> Result<(String, Vec<u8>)> {
        let config: Value = serde_json::from_slice(&decider_2b("decider_config.json")?)?;
        let profile = serde_json::to_string(&from_decider_config(&config, Layout::Plain)?)?;
        Ok((profile, decider_2b("tokenizer.json")?))
    }

    fn ticket() -> Result<String> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/requests/ticket.json");
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
    }

    /// Logits that differ by row and label, the same for the same row and labels.
    fn logits(row: usize, labels: usize) -> Vec<f32> {
        (0..labels)
            .map(|label| ((row * 7 + label * 3) % 5) as f32 - 2.0)
            .collect()
    }

    /// A model answering every row with [`logits`].
    struct Rows(usize);

    impl LoadedModel for Rows {
        fn n_ctx(&self) -> usize {
            LoadOptions::default().n_ctx as usize
        }

        fn slot_logits(
            &mut self,
            _: &[u32],
            slots: &[usize],
            label_ids: &[u32],
        ) -> anyhow::Result<Vec<Vec<f32>>> {
            self.0 += 1;
            Ok(vec![logits(self.0 - 1, label_ids.len()); slots.len()])
        }
    }

    /// A run in the tab hands out the rows the server would decode, reads its answers out of their logits as the server
    /// does, and answers a refused request, or a row that did not decode, with the server's own bytes.
    #[test]
    fn runs_answer_as_the_server() -> Result<()> {
        let (profile_json, tokenizer) = decider_2b_files()?;
        let profile = profile(&profile_json).map_err(anyhow::Error::msg)?;
        let reader = reader("decider:2b", &tokenizer, &profile).map_err(anyhow::Error::msg)?;
        let decider = reader.0.clone();
        let body = ticket()?;
        let request: SystemOneRequest = serde_json::from_str(&body)?;

        assert!(
            reader
                .plan(r#"{"questions":{}}"#)
                .is_err_and(|err| err.starts_with("the request is not a /v1/systemone body: ")),
        );
        let mut run = reader.plan(&body).map_err(anyhow::Error::msg)?;
        assert!(!run.refused());
        let plan = decider.plan(&request)?;
        assert_eq!(run.rows(), plan.rows());
        for (row, decode) in decider.decodes(&plan).enumerate() {
            assert_eq!(run.ids(row), decode.ids, "row {row}");
            assert_eq!(run.labels(row), decode.label_ids, "row {row}");
            run.push(&logits(row, decode.label_ids.len()));
        }
        let server = decider.decide(&mut Rows(0), &request)?;
        assert_eq!((run.status(), run.body()), (200, body_text(&server)));

        // A question the server refuses: its 422, before any row is decoded.
        let mut invalid: Value = serde_json::from_str(&body)?;
        invalid["questions"]["department"]["criteria"] = serde_json::json!(["billing"]);
        let invalid = invalid.to_string();
        let refused = reader.plan(&invalid).map_err(anyhow::Error::msg)?;
        let err = decider
            .plan(&serde_json::from_str(&invalid)?)
            .expect_err("one option is refused");
        assert!(refused.refused());
        assert_eq!(refused.rows(), 0);
        assert_eq!(
            (refused.status(), refused.body()),
            (422, body_text(&err.body()))
        );

        // A row that did not decode: the server's answer to a runtime that failed.
        let mut failed = reader.plan(&body).map_err(anyhow::Error::msg)?;
        failed.push(&logits(0, failed.labels(0).len()));
        failed.fail("onnxruntime-web: the device was lost");
        assert_eq!(
            (failed.status(), failed.body()),
            (
                500,
                r#"{"detail":{"error_type":"api_error","message":"onnxruntime-web: the device was lost"}}"#
                    .to_string()
            )
        );
        Ok(())
    }

    /// What the tab cannot read is said as the reason the raw exchange shows.
    #[test]
    fn unreadable_files_say_why() -> Result<()> {
        let (profile_json, _) = decider_2b_files()?;
        let read = profile(&profile_json).map_err(anyhow::Error::msg)?;
        assert!(
            profile("{")
                .is_err_and(|err| err.starts_with("the server sent an unreadable profile: ")),
        );
        assert!(
            reader("decider:2b", b"{", &read)
                .is_err_and(|err| err.starts_with("the tokenizer of decider:2b does not load: ")),
        );
        Ok(())
    }
}

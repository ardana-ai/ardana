//! `ardana push`: a model `ardana finetune` made, uploaded to a Hugging Face repository so `ardana pull
//! hf.co/<org>/<repo>` runs it anywhere (Q12, Q28). The repository gets the model's directory as it is, but its
//! `finetune.json` (a GGUF named `<name>-<QUANT>.gguf`, or `config.json` and the safetensors Candle runs, the tokenizer
//! and chat template files and `decider_config.json`, all at the root, where `ardana pull` reads them), and a model
//! card, `README.md`, made from `finetune.json`: the base's license, its checkpoint as `base_model`, the commands that
//! pull and run the model, the Q23 settings and the test report. The repository is private unless `--public` (Q19);
//! `--dry-run` prints what would be sent and sends nothing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ardana_api::human_size;
use ardana_registry::Registry;
use ardana_registry::base::CARD;
use ardana_registry::hub::{self, Upload};
use ardana_registry::refs::{HF_PREFIX, Ref};
use clap::Args;
use serde_json::Value;

use crate::finetune::{self, LEARNING_RATE, LORA_ALPHA, LORA_RANK, RECORD, SEED};
use crate::render;

#[derive(Debug, Args)]
pub struct PushArgs {
    /// A model `ardana finetune` made, as `ardana list` names it
    #[arg(value_name = "MODEL")]
    pub name: String,
    /// The Hugging Face repository to push to, hf.co/<org>/<repo>; created when it does not exist
    #[arg(value_name = "REPOSITORY")]
    pub repository: String,
    /// Make the repository public; by default it is private
    #[arg(long)]
    pub public: bool,
    /// Print the repository, every file with its bytes and the model card, and send nothing
    #[arg(long)]
    pub dry_run: bool,
}

/// `ardana push`: the target and the model checked, then what is pushed printed; without `--dry-run`, the token
/// found and the files uploaded in one commit.
pub fn push(args: &PushArgs) -> Result<()> {
    let (org, repo) = match Ref::parse(&args.repository) {
        Ok(Ref::Hf {
            org,
            repo,
            file: None,
        }) => (org, repo),
        _ => bail!(
            "{} is no Hugging Face repository; push to {HF_PREFIX}<org>/<repo>, without a quant or file",
            args.repository
        ),
    };
    let target = format!("{HF_PREFIX}{org}/{repo}");
    let home = Registry::home()?;
    let registry = Registry::open(&home)?;
    let model = registry.entry(&args.name)?;
    let Some(dir) = finetune::made(&home, model)? else {
        bail!(
            "{} was not made by `ardana finetune`: {} holds no {RECORD}, and ardana push uploads only the models \
             ardana finetune makes",
            model.name,
            home.join(finetune::MODELS_DIR).join(&model.name).display()
        );
    };
    let path = dir.join(RECORD);
    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
    )
    .with_context(|| format!("parsing {}", path.display()))?;
    let card = card(&model.name, &record, &target)
        .with_context(|| format!("writing the model card from {}", path.display()))?;
    let files = files(&dir)?;
    // A push needs a token before anything is printed or sent; a dry run needs none.
    let token = match (args.dry_run, hub::token()) {
        (true, _) => None,
        (false, Some(token)) => Some(token),
        (false, None) => bail!(
            "no Hugging Face token to push with: neither {} nor {} holds one; set {} to a token that can write to \
             {org}, or run `hf auth login`",
            hub::TOKEN_VAR,
            hub::token_path().display(),
            hub::TOKEN_VAR
        ),
    };

    let visibility = if args.public { "public" } else { "private" };
    let mut rows: Vec<Vec<String>> = files
        .iter()
        .map(|(file, _, bytes)| vec![file.clone(), bytes.to_string()])
        .collect();
    rows.push(vec![CARD.to_string(), card.len().to_string()]);
    let total = files.iter().map(|(_, _, bytes)| bytes).sum::<u64>() + card.len() as u64;
    println!(
        "{} to {target} ({visibility}){}",
        model.name,
        if args.dry_run {
            "; a dry run, nothing is sent"
        } else {
            ""
        }
    );
    print!("{}", render::table(&["FILE", "BYTES"], &rows));
    println!("{} files, {}", rows.len(), human_size(total));
    println!("\n{CARD}:\n{card}");
    let Some(token) = token else {
        return Ok(());
    };

    let mut uploads: Vec<(String, Upload)> = files
        .into_iter()
        .map(|(file, path, _)| (file, Upload::File(path)))
        .collect();
    uploads.push((CARD.to_string(), Upload::Bytes(card.into_bytes())));
    let message = format!("Push {} made by ardana finetune", model.name);
    let pushed = crate::block_on(hub::push(
        &org,
        &repo,
        args.public,
        token,
        uploads,
        &message,
    ))??;
    println!(
        "pushed {} to {target} ({visibility}{}){}; `{}` runs it",
        model.name,
        if pushed.created { ", created" } else { "" },
        pushed
            .commit_url
            .map(|url| format!(": {url}"))
            .unwrap_or_default(),
        pull_command(&record, &target)?
    );
    Ok(())
}

/// The files of the model's directory `dir` a push uploads, by name: every file but `finetune.json`, whose content the
/// card carries, with its path and bytes.
fn files(dir: &Path) -> Result<Vec<(String, PathBuf, u64)>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        let Some(file) = path.file_name().and_then(|f| f.to_str()) else {
            bail!("{} is no UTF-8 file name", path.display());
        };
        if file == RECORD || !path.is_file() {
            continue;
        }
        let bytes = std::fs::metadata(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .len();
        files.push((file.to_string(), path, bytes));
    }
    files.sort();
    Ok(files)
}

/// The string `finetune.json` holds at `key`.
fn text<'a>(record: &'a Value, key: &str) -> Result<&'a str> {
    record[key]
        .as_str()
        .with_context(|| format!("{RECORD} holds no {key}"))
}

/// The command that pulls the model pushed to `target`: a GGUF by its quant, as `ardana finetune` names its file
/// (`<name>-<QUANT>.gguf`), a checkpoint by the repository alone.
fn pull_command(record: &Value, target: &str) -> Result<String> {
    Ok(match text(record, "format")? {
        "gguf" => format!(
            "ardana pull {target}:{}",
            text(record, "quant")?.to_ascii_uppercase()
        ),
        _ => format!("ardana pull {target}"),
    })
}

/// A base as a public card shows it: a local file by its name alone, any other name as it is.
fn shown(base: &str) -> String {
    match Ref::parse(base) {
        Ok(Ref::Local(path)) => path
            .file_name()
            .map_or_else(|| base.to_string(), |f| f.to_string_lossy().into_owned()),
        _ => base.to_string(),
    }
}

/// The Hub repository the checkpoint of `finetune.json` is, `<org>/<repo>`, and its commit when recorded: the
/// `{"repo", "commit"}` form's, or a `{"path"}` in the hub cache's layout, `models--<org>--<repo>/snapshots/<commit>`.
/// `None` for a directory of any other kind.
fn checkpoint_repo(checkpoint: &Value) -> Option<(String, String)> {
    if let (Some(repo), Some(commit)) = (checkpoint["repo"].as_str(), checkpoint["commit"].as_str())
    {
        return Some((
            repo.strip_prefix(HF_PREFIX).unwrap_or(repo).to_string(),
            commit.to_string(),
        ));
    }
    let path = Path::new(checkpoint["path"].as_str()?);
    let commit = path.file_name()?.to_str()?;
    let snapshots = path.parent()?;
    let (org, repo) = snapshots
        .parent()?
        .file_name()?
        .to_str()?
        .strip_prefix("models--")?
        .split_once("--")?;
    (snapshots.file_name()? == "snapshots").then(|| (format!("{org}/{repo}"), commit.to_string()))
}

/// The model card of the model `name` with `finetune.json` `record`, pushed to `target`: front matter with the base's
/// `license`, `base_model` (the checkpoint's repository) and `base_model_relation: finetune`, and the tags `ardana`,
/// `system-one` and the format (Q28, R6.2); a body with the commands that pull and run it, how it was trained (Q23) and
/// the test report.
fn card(name: &str, record: &Value, target: &str) -> Result<String> {
    let format = text(record, "format")?;
    let base = shown(text(record, "base")?);
    let checkpoint = checkpoint_repo(&record["checkpoint"]);
    let license = record["license"].as_str();
    let mut front = vec!["---".to_string()];
    if let Some(license) = license {
        // The Hub's license identifiers are lowercase (`apache-2.0`); the library writes SPDX's (`Apache-2.0`).
        front.push(format!("license: {}", license.to_ascii_lowercase()));
    }
    if let Some((repo, _)) = &checkpoint {
        front.push(format!("base_model: {repo}"));
        front.push("base_model_relation: finetune".to_string());
    }
    front.extend([
        "tags:".to_string(),
        "- ardana".to_string(),
        "- system-one".to_string(),
        format!("- {format}"),
        "---".to_string(),
    ]);

    let from = match &checkpoint {
        Some((repo, commit)) => format!(
            "the checkpoint [{repo}](https://huggingface.co/{repo}) at revision \
             [`{commit}`](https://huggingface.co/{repo}/tree/{commit})"
        ),
        None => "a checkpoint on the trainer's own disk".to_string(),
    };
    let dtype = text(record, "dtype")?;
    let written = match format {
        "gguf" => {
            let quant = text(record, "quant")?.to_ascii_uppercase();
            format!("merged and quantized to {quant} (`{name}-{quant}.gguf`)")
        }
        _ => format!("merged into a safetensors checkpoint in {dtype}, which Candle runs"),
    };
    let run_name = Ref::parse(target)?.default_name();
    let license = match license {
        Some(license) => format!("The license is the base's: {license}."),
        None => "The base names no license.".to_string(),
    };
    let rows = &record["rows"];
    let count = |key: &str| {
        rows[key]
            .as_u64()
            .with_context(|| format!("{RECORD} holds no rows.{key}"))
    };
    let epochs = record["epochs"]
        .as_u64()
        .with_context(|| format!("{RECORD} holds no epochs"))?;
    let qlora = if record["qlora"].as_bool() == Some(true) {
        ", its base in 4-bit NF4 (QLoRA)"
    } else {
        ""
    };
    let mut body = format!(
        "# {name}\n\n\
         A System One model: it reads a state once and answers typed questions (noul, choice, score) with calibrated \
         probabilities. `ardana finetune` made it from {base}, {from}: a LoRA trained on labelled lines, {written}, \
         its temperatures fitted on lines held out from training. {license}\n\n\
         Run it with [Ardana](https://ardana.ai), a `/v1/systemone` request in `request.json`:\n\n\
         ```sh\n{}\nardana run {run_name} --request request.json\n```\n\n\
         ## Training\n\n\
         - A LoRA of rank {LORA_RANK} and alpha {LORA_ALPHA} on every linear layer, learning rate {LEARNING_RATE}, \
         {epochs} epoch{}, seed {SEED}; the epoch with the lowest calibration loss kept.\n\
         - On {} in {dtype}{qlora}.\n\
         - {} lines to train on, {} to calibrate on, {} to test on.\n",
        pull_command(record, target)?,
        if epochs == 1 { "" } else { "s" },
        text(record, "device")?,
        count("train")?,
        count("calibration")?,
        count("test")?,
    );
    body.push_str(&report(&record["report"])?);
    Ok(format!("{}\n\n{body}", front.join("\n")))
}

/// The test report of `finetune.json` as a Markdown table: a row per answer type and model, then the overall rows.
fn report(report: &Value) -> Result<String> {
    let lines = report["lines"]
        .as_u64()
        .with_context(|| format!("{RECORD} holds no test report"))?;
    let mut text = format!(
        "\n## Test report\n\n\
         The base and this model on the {lines} test lines (a noul answered yes at P(yes) ≥ 0.5; NLL of the gold \
         option, Brier score, 15-bin ECE):\n\n\
         | Type | Model | Answers | Accuracy | NLL | Brier | ECE |\n\
         |------|-------|---------|----------|-----|-------|-----|\n"
    );
    let tuned = report["tuned"]["metrics"]
        .as_object()
        .with_context(|| format!("{RECORD}'s report holds no tuned metrics"))?;
    for kind in tuned.keys() {
        for model in [&report["base"], &report["tuned"]] {
            let name = model["name"]
                .as_str()
                .with_context(|| format!("{RECORD}'s report names no model"))?;
            let m = &model["metrics"][kind];
            let number = |key: &str| {
                m[key]
                    .as_f64()
                    .with_context(|| format!("{RECORD}'s report holds no {kind} {key} of {name}"))
            };
            let answers = m["answers"]
                .as_u64()
                .with_context(|| format!("{RECORD}'s report holds no {kind} answers of {name}"))?;
            text.push_str(&format!(
                "| {kind} | {} | {answers} | {:.4} | {:.4} | {:.4} | {:.4} |\n",
                shown(name),
                number("accuracy")?,
                number("nll")?,
                number("brier")?,
                number("ece")?
            ));
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `{"path"}` checkpoint is a repository only in the hub cache's layout.
    #[test]
    fn checkpoint_repositories() {
        let pinned = serde_json::json!({"repo": "hf.co/Qwen/Qwen3-0.6B", "commit": "c1"});
        assert_eq!(
            checkpoint_repo(&pinned),
            Some(("Qwen/Qwen3-0.6B".into(), "c1".into()))
        );
        let cached = serde_json::json!({"path": "/x/hub/models--Qwen--Qwen3-0.6B/snapshots/c2"});
        assert_eq!(
            checkpoint_repo(&cached),
            Some(("Qwen/Qwen3-0.6B".into(), "c2".into()))
        );
        for other in ["/x/models--Qwen--Qwen3-0.6B/c2", "/x/checkpoint", "/"] {
            assert_eq!(checkpoint_repo(&serde_json::json!({"path": other})), None);
        }
    }
}

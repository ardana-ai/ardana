//! `cargo xtask onnx convert|publish <name>`: a library model's browser variant, ONNX weights for onnxruntime-web, and
//! the GGUF ardana-ai hosts for it, both built from the checkpoint `library.toml` names as the model's `source` (Q6,
//! `docs/guidelines/onnx.md`); never in CI.
//!
//! `convert` downloads the checkpoint into `tmp/hf`, exports the ONNX with `xtask/scripts/onnx_export.py` in the
//! `tmp/py/onnx` venv it creates, converts the GGUF with llama.cpp's `convert_hf_to_gguf.py` when the model's `weights`
//! is an ardana-ai repository, writes each repository into the sandbox Hub cache as the snapshot `publish` uploads,
//! records the sizes it built in `library.toml` and reads both repositories back offline with the release
//! `ardana pull`. `publish` prints its `hf` commands, the model cards and the `[[hf]]` entries, then (without
//! `--dry-run`) uploads the snapshots to huggingface.co/ardana-ai with the user's token and pins them in
//! `xtask/fetch.toml`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::env::quote;
use crate::fetch::{Hf, Manifest, blob_name, link_snapshot, remove_if_present};
use crate::python::pinned_venv;
use crate::sandbox::{Sandbox, run};
use crate::serve::build_ardana;

/// The Hugging Face organisation that hosts what `convert` builds.
const ORG: &str = "ardana-ai";
/// The model library, which names what to build and records the sizes built.
pub(crate) const LIBRARY: &str = "crates/ardana-registry/src/library.toml";
/// The step's venv, relative to the repo root, its Python and its pins: the model builder and what it runs on, and
/// the Hub client whose `hf` downloads checkpoints and uploads repositories. llama.cpp's converter runs in it too.
const VENV: &str = "tmp/py/onnx";
const PYTHON: &str = "3.12";
const PACKAGES: &[&str] = &[
    "onnxruntime-genai==0.17.1",
    "onnxruntime==1.30.0",
    "onnx==1.23.1",
    "onnx-ir==1.0.0",
    "torch==2.14.1",
    "transformers==5.18.0",
    "tokenizers==0.23.2",
    "safetensors==0.8.0",
    "huggingface-hub==1.33.0",
    "numpy==2.5.3",
    "ml-dtypes==0.6.0",
    "protobuf==7.36.2",
];
const EXPORT_SCRIPT: &str = "xtask/scripts/onnx_export.py";
/// The `xtask/fetch.toml` research clone whose `convert_hf_to_gguf.py` writes the GGUFs.
const LLAMA_CPP: &str = "llama.cpp";
/// The GGUF types `convert_hf_to_gguf.py` writes itself (`--outtype`).
const GGUF_TYPES: [&str; 4] = ["f32", "f16", "bf16", "q8_0"];
/// The exported graph and its external data.
const ONNX_FILES: [&str; 2] = ["model.onnx", "model.onnx.data"];
/// The tokenizer, which a browser downloads with the graph.
const TOKENIZER: &str = "tokenizer.json";
/// The checkpoint's files both repositories carry unchanged when it has them: the tokenizer, the special tokens and
/// chat template the chat layout reads, decider's config and the license.
const SOURCE_FILES: [&str; 5] = [
    TOKENIZER,
    "tokenizer_config.json",
    "chat_template.jinja",
    "decider_config.json",
    "LICENSE",
];
/// The model card. It and the license are the files `publish` leaves out of the `[[hf]]` pins: nothing reads them.
const CARD: &str = "README.md";
const UNPINNED: [&str; 2] = [CARD, "LICENSE"];
/// The Hub stores a file with Git LFS when its default `.gitattributes` names it (`*.onnx`) or the file is larger than
/// this (10 MB, as its documentation puts it); the cache then names its blob by the sha256, else by the git blob id,
/// as `cargo xtask fetch` does.
const LFS_SIZE: u64 = 10_000_000;

/// The part of `library.toml` the step reads; `ardana-registry` validates the whole file.
#[derive(Debug, Deserialize)]
struct LibraryFile {
    #[serde(rename = "model")]
    models: Vec<LibraryModel>,
}

#[derive(Debug, Deserialize)]
struct LibraryModel {
    name: String,
    weights: String,
    quant: String,
    source: Option<String>,
    browser: Option<BrowserTable>,
}

#[derive(Debug, Deserialize)]
struct BrowserTable {
    weights: String,
    quant: String,
}

/// What `library.toml` says to build for one model.
#[derive(Debug)]
struct Target {
    name: String,
    /// The checkpoint, `org/repo` at a commit.
    source: Hf,
    /// The ONNX repository, `ardana-ai/<repo>`, and its export recipe (`int4` or `int8`).
    onnx: String,
    onnx_quant: String,
    /// The GGUF repository ardana-ai hosts and the GGUF's quant, when the model's `weights` is one.
    gguf: Option<(String, String)>,
}

impl Target {
    fn load(repo_root: &Path, name: &str) -> Result<Target> {
        let path = repo_root.join(LIBRARY);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let library: LibraryFile =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let Some(model) = library
            .models
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
        else {
            let names: Vec<&str> = library.models.iter().map(|m| m.name.as_str()).collect();
            bail!(
                "no library model {name}; the library has {}",
                names.join(", ")
            );
        };
        let Some(browser) = &model.browser else {
            bail!(
                "library model {} has no browser variant: {LIBRARY} gives it no [model.browser] table (weights, \
                 quant)",
                model.name
            );
        };
        let source = model.source.as_deref().with_context(|| {
            format!(
                "library model {} names no `source` checkpoint in {LIBRARY}",
                model.name
            )
        })?;
        let source = source
            .split_once('@')
            .and_then(|(repo, revision)| {
                Some(Hf {
                    repo: hf_repo(repo)?.to_string(),
                    revision: revision.to_string(),
                    files: Vec::new(),
                })
            })
            .with_context(|| {
                format!(
                    "the source {source:?} of {} is not hf.co/<org>/<repo>@<commit>",
                    model.name
                )
            })?;
        let onnx = hf_repo(&browser.weights)
            .filter(|repo| hosted(repo))
            .with_context(|| {
                format!(
                    "the browser weights {:?} of {} are no hf.co/{ORG}/<repo> repository",
                    browser.weights, model.name
                )
            })?;
        let gguf = match hf_repo(&model.weights).filter(|repo| hosted(repo)) {
            Some(repo) if GGUF_TYPES.contains(&model.quant.to_lowercase().as_str()) => {
                Some((repo.to_string(), model.quant.clone()))
            }
            Some(repo) => bail!(
                "{repo} hosts a {} GGUF, but convert_hf_to_gguf.py writes {} only",
                model.quant,
                GGUF_TYPES.join(", ")
            ),
            None => None,
        };
        Ok(Target {
            name: model.name.clone(),
            source,
            onnx: onnx.to_string(),
            onnx_quant: browser.quant.clone(),
            gguf,
        })
    }

    /// The repositories `convert` writes and `publish` uploads: the ONNX one, then the GGUF one.
    fn repos(&self) -> Vec<&str> {
        std::iter::once(self.onnx.as_str())
            .chain(self.gguf.as_ref().map(|(repo, _)| repo.as_str()))
            .collect()
    }
}

/// `org/repo` of the reference `hf.co/<org>/<repo>`.
fn hf_repo(reference: &str) -> Option<&str> {
    let repo = reference.strip_prefix("hf.co/")?;
    let (org, name) = repo.split_once('/')?;
    (!org.is_empty() && !name.is_empty() && !name.contains(['/', ':'])).then_some(repo)
}

/// Whether ardana-ai hosts the repository `org/repo`.
fn hosted(repo: &str) -> bool {
    repo.split_once('/').is_some_and(|(org, _)| org == ORG)
}

pub fn convert(sandbox: &Sandbox, name: &str) -> Result<()> {
    let root = sandbox.repo_root();
    let target = Target::load(root, name)?;
    // The GGUF's converter is checked before the long export.
    let gguf_job = match &target.gguf {
        Some((repo, quant)) => Some((repo, quant, llama_cpp(sandbox)?)),
        None => None,
    };
    let work = sandbox.tmp().join("onnx").join(&target.name);
    remove_if_present(&work)?;
    std::fs::create_dir_all(&work)?;
    let python = pinned_venv(sandbox, &root.join(VENV), PYTHON, PACKAGES)?;
    let checkpoint = download(sandbox, &target.source)?;
    let license = license(&checkpoint)?;
    let offline = |cmd: &mut std::process::Command| {
        cmd.current_dir(root)
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1");
    };

    let export = work.join("export");
    let mut cmd = sandbox.command(&python);
    offline(&mut cmd);
    run(cmd
        .arg(root.join(EXPORT_SCRIPT))
        .arg(&checkpoint)
        .arg(&target.onnx_quant)
        .arg(&export)
        .arg(work.join("cache")))?;
    let staged = stage(&work, &target.onnx, &checkpoint)?;
    for file in ONNX_FILES {
        std::fs::rename(export.join(file), staged.join(file))
            .with_context(|| format!("moving the exported {file}"))?;
    }
    let card = onnx_card(&target, &license, &files_in(&staged)?);
    std::fs::write(staged.join(CARD), card)?;
    let onnx = write_repo(sandbox, &target.onnx, &staged)?;
    let browser_size = [ONNX_FILES[0], ONNX_FILES[1], TOKENIZER]
        .iter()
        .map(|file| file_size(&snapshot_dir(sandbox, &onnx).join(file)))
        .sum::<Result<u64>>()?;
    println!(
        "onnx: wrote hf.co/{}@{} ({} {}: {browser_size} bytes for a browser)",
        onnx.repo, onnx.revision, target.name, target.onnx_quant
    );

    let mut gguf = None;
    if let Some((repo, quant, llama_cpp)) = gguf_job {
        let staged = stage(&work, repo, &checkpoint)?;
        let file = format!("{}-{quant}.gguf", target.name);
        let mut cmd = sandbox.command(&python);
        offline(&mut cmd);
        run(cmd
            .arg(llama_cpp.dir.join("convert_hf_to_gguf.py"))
            .arg(&checkpoint)
            .args(["--outtype", &quant.to_lowercase(), "--no-nextn"])
            .args(["--model-name", &target.name, "--outfile"])
            .arg(staged.join(&file)))?;
        let card = gguf_card(
            &target,
            &license,
            &llama_cpp.version,
            (&file, quant),
            &files_in(&staged)?,
        );
        std::fs::write(staged.join(CARD), card)?;
        let hf = write_repo(sandbox, repo, &staged)?;
        let size = file_size(&snapshot_dir(sandbox, &hf).join(&file))?;
        println!(
            "onnx: wrote hf.co/{}@{} ({file}: {size} bytes)",
            hf.repo, hf.revision
        );
        gguf = Some((hf, size));
    }

    let library = root.join(LIBRARY);
    let text = std::fs::read_to_string(&library)?;
    let recorded = record_sizes(
        &text,
        &target.name,
        browser_size,
        gguf.as_ref().map(|(_, size)| *size),
    )?;
    std::fs::write(&library, recorded)?;
    println!("onnx: recorded the sizes in {LIBRARY}");
    read_back(
        sandbox,
        &target,
        &work.join("home"),
        &onnx,
        gguf.as_ref().map(|(hf, _)| hf),
    )
}

/// The llama.cpp clone at its `xtask/fetch.toml` pin.
struct LlamaCpp {
    dir: PathBuf,
    /// The release tag the pin names (`b11074`).
    version: String,
}

fn llama_cpp(sandbox: &Sandbox) -> Result<LlamaCpp> {
    let manifest = Manifest::load(sandbox.repo_root())?;
    let tool = manifest
        .tool
        .iter()
        .find(|tool| tool.name == LLAMA_CPP)
        .with_context(|| format!("xtask/fetch.toml has no [[tool]] {LLAMA_CPP}"))?;
    tool.check(sandbox, &tool.version)
        .context("run `cargo xtask fetch`")?;
    Ok(LlamaCpp {
        dir: tool.git_dir(sandbox),
        version: tool.version.clone(),
    })
}

/// The checkpoint's snapshot in the sandbox Hub cache, downloaded at its commit unless it is there already.
fn download(sandbox: &Sandbox, source: &Hf) -> Result<PathBuf> {
    let root = sandbox.repo_root();
    run(sandbox
        .command(root.join(VENV).join("bin/hf"))
        .current_dir(root)
        .args(["download", &source.repo, "--revision", &source.revision])
        .args(["--format", "quiet"]))?;
    let snapshot = snapshot_dir(sandbox, source);
    if !snapshot.join("config.json").is_file() {
        bail!(
            "{} has no config.json after downloading {}@{}",
            snapshot.display(),
            source.repo,
            source.revision
        );
    }
    Ok(snapshot)
}

/// `snapshots/<revision>` of `hf` in the sandbox Hub cache.
fn snapshot_dir(sandbox: &Sandbox, hf: &Hf) -> PathBuf {
    hf.repo_dir(sandbox).join("snapshots").join(&hf.revision)
}

/// The license the checkpoint's model card declares in its front matter.
fn license(checkpoint: &Path) -> Result<String> {
    let path = checkpoint.join(CARD);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    text.strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---"))
        .and_then(|(front, _)| front.lines().find_map(|line| line.strip_prefix("license:")))
        .map(|license| license.trim().to_string())
        .filter(|license| !license.is_empty())
        .with_context(|| format!("{} declares no license", path.display()))
}

/// `work/<repo name>`, holding the checkpoint's [`SOURCE_FILES`]; the caller adds the rest of the repository.
fn stage(work: &Path, repo: &str, checkpoint: &Path) -> Result<PathBuf> {
    let name = repo.rsplit('/').next().unwrap_or(repo);
    let dir = work.join(name);
    std::fs::create_dir_all(&dir)?;
    for file in SOURCE_FILES {
        let from = checkpoint.join(file);
        if from.is_file() {
            std::fs::copy(&from, dir.join(file))
                .with_context(|| format!("copying {}", from.display()))?;
        }
    }
    Ok(dir)
}

/// The names of the files in `dir`, sorted.
fn files_in(dir: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("listing {}", dir.display()))? {
        let entry = entry?;
        if entry.path().is_file() {
            files.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    files.sort();
    Ok(files)
}

fn file_size(path: &Path) -> Result<u64> {
    Ok(std::fs::metadata(path)
        .with_context(|| format!("{} is missing", path.display()))?
        .len())
}

/// Moves the files of `staged` into the sandbox Hub cache as one snapshot of `repo`, in the layout `hf-hub` reads
/// and `cargo xtask fetch` writes: `blobs/<sha256 or git blob id>`, `snapshots/<commit>/<file>` linking to it and
/// `refs/main` holding the commit. The commit is local until `publish`: the git blob id of the snapshot's manifest
/// (each file's blob and name), so the same files make the same snapshot.
fn write_repo(sandbox: &Sandbox, repo: &str, staged: &Path) -> Result<Hf> {
    let files = files_in(staged)?;
    let mut manifest = String::new();
    let mut blobs = Vec::new();
    for file in &files {
        let path = staged.join(file);
        let lfs = file.ends_with(".onnx") || file_size(&path)? > LFS_SIZE;
        let blob = blob_name(sandbox, &path, lfs)?;
        manifest.push_str(&format!("{blob} {file}\n"));
        blobs.push(blob);
    }
    let manifest_path = staged.join(".manifest");
    std::fs::write(&manifest_path, manifest)?;
    let hf = Hf {
        repo: repo.to_string(),
        revision: blob_name(sandbox, &manifest_path, false)?,
        files,
    };
    let dir = hf.repo_dir(sandbox);
    std::fs::create_dir_all(dir.join("blobs"))?;
    for (file, blob) in hf.files.iter().zip(&blobs) {
        let path = dir.join("blobs").join(blob);
        if path.is_file() {
            std::fs::remove_file(staged.join(file))?;
        } else {
            std::fs::rename(staged.join(file), &path)
                .with_context(|| format!("moving {file} into {}", path.display()))?;
        }
        link_snapshot(&snapshot_dir(sandbox, &hf), file, blob)?;
    }
    std::fs::create_dir_all(dir.join("refs"))?;
    std::fs::write(dir.join("refs/main"), &hf.revision)?;
    hf.check(sandbox)?;
    Ok(hf)
}

/// The version `PACKAGES` pins `package` at.
fn pinned(package: &str) -> &'static str {
    PACKAGES
        .iter()
        .find_map(|pin| pin.strip_prefix(package)?.strip_prefix("=="))
        .unwrap_or_default()
}

/// `` `a`, `b` `` for the copied source files among `files`.
fn copied(files: &[String]) -> String {
    let names: Vec<String> = files
        .iter()
        .filter(|file| SOURCE_FILES.contains(&file.as_str()))
        .map(|file| format!("`{file}`"))
        .collect();
    names.join(", ")
}

/// The front matter and the provenance line every card starts with.
fn card_head(target: &Target, license: &str, tag: &str, title: &str) -> String {
    let source = &target.source;
    format!(
        "---\nlicense: {license}\nbase_model: {repo}\nbase_model_relation: quantized\ntags:\n- ardana\n- {tag}\n---\n\n\
         # {title}\n\n\
         Built from [{repo}](https://huggingface.co/{repo}) at revision \
         [`{revision}`](https://huggingface.co/{repo}/tree/{revision}) by `cargo xtask onnx convert {name}` in the \
         Ardana repository. The license is the source's: {license}.\n\n",
        repo = source.repo,
        revision = source.revision,
        name = target.name,
    )
}

/// The ONNX repository's model card.
fn onnx_card(target: &Target, license: &str, files: &[String]) -> String {
    let title = format!(
        "{} for the browser ({} ONNX)",
        target.name, target.onnx_quant
    );
    format!(
        "{}The browser variant of Ardana's library model `{}`, for onnxruntime-web.\n\n\
         - `model.onnx`, `model.onnx.data`: the graph and its {} weights, exported by the onnxruntime-genai {} model \
         builder for the WebGPU execution provider: fp16 inputs and outputs, the logits of the last position only, \
         no multi-token-prediction head, the embedding shared with the LM head.\n\
         - {}: the source's, unchanged.\n",
        card_head(target, license, "onnx", &title),
        target.name,
        target.onnx_quant,
        pinned("onnxruntime-genai"),
        copied(files),
    )
}

/// The GGUF repository's model card, for its `gguf` file in `quant`.
fn gguf_card(
    target: &Target,
    license: &str,
    llama_cpp: &str,
    (gguf, quant): (&str, &str),
    files: &[String],
) -> String {
    let title = format!("{} GGUF ({quant})", target.name);
    format!(
        "{}Ardana's library model `{name}`: `ardana run {name}` runs it.\n\n\
         - `{gguf}`: the weights in {quant}, converted by llama.cpp {llama_cpp}'s \
         `convert_hf_to_gguf.py --outtype {} --no-nextn` (no multi-token-prediction head).\n\
         - {}: the source's, unchanged.\n",
        card_head(target, license, "gguf", &title),
        quant.to_lowercase(),
        copied(files),
        name = target.name,
    )
}

/// `library.toml` with the sizes `convert` built for `name`: its browser variant's, and the model's own when it
/// built the GGUF. Every other line stays as it is; a browser table without `size` gets it after its last line.
fn record_sizes(text: &str, name: &str, browser: u64, gguf: Option<u64>) -> Result<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let name_line = format!("name = \"{name}\"");
    let at = lines
        .iter()
        .position(|line| line.trim() == name_line)
        .with_context(|| format!("{LIBRARY} has no line {name_line}"))?;
    let start = lines[..at]
        .iter()
        .rposition(|line| line.trim() == "[[model]]")
        .with_context(|| format!("{name_line} is in no [[model]] table"))?;
    let end = lines[at..]
        .iter()
        .position(|line| line.trim() == "[[model]]")
        .map_or(lines.len(), |offset| at + offset);
    let browser_at = (start..end)
        .find(|&i| lines[i].trim() == "[model.browser]")
        .with_context(|| format!("the {name} model has no [model.browser] table"))?;
    let size = |n: u64| format!("size = {}", grouped(n));
    if let Some(gguf) = gguf {
        let i = (start..browser_at)
            .find(|&i| lines[i].starts_with("size = "))
            .with_context(|| format!("the {name} model has no size"))?;
        lines[i] = size(gguf);
    }
    match (browser_at..end).find(|&i| lines[i].starts_with("size = ")) {
        Some(i) => lines[i] = size(browser),
        None => {
            let last = (browser_at..end)
                .rev()
                .find(|&i| !lines[i].trim().is_empty())
                .unwrap_or(browser_at);
            lines.insert(last + 1, size(browser));
        }
    }
    Ok(lines.join("\n") + "\n")
}

/// `1_234_567`, as `library.toml` writes byte counts.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('_');
        }
        out.push(digit);
    }
    out
}

/// Reads both repositories back offline through `ardana_registry`'s Hub: the release `ardana pull`s the model into the
/// scratch `home` with the ONNX repository's tokenizer; its entry must point into the snapshots just written (the GGUF
/// one when `convert` built it, else the library's GGUF from `cargo xtask fetch`).
fn read_back(
    sandbox: &Sandbox,
    target: &Target,
    home: &Path,
    onnx: &Hf,
    gguf: Option<&Hf>,
) -> Result<()> {
    let ardana = build_ardana(sandbox)?;
    let tokenizer = format!("hf.co/{}", onnx.repo);
    run(sandbox
        .command(&ardana)
        .env("ARDANA_HOME", home)
        .env("HF_HUB_OFFLINE", "1")
        .args(["pull", &target.name, "--tokenizer", &tokenizer]))
    .context(
        "reading the repositories back offline (the library's GGUFs come from `cargo xtask fetch`)",
    )?;
    let path = home.join("models.toml");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let registry: toml::Value =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let entry = registry
        .get("model")
        .and_then(toml::Value::as_array)
        .and_then(|models| models.first())
        .with_context(|| format!("{} records no model", path.display()))?;
    let mut expect = vec![("tokenizer", onnx)];
    expect.extend(gguf.map(|hf| ("weights", hf)));
    for (field, hf) in expect {
        let file = entry
            .get(field)
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        if !Path::new(file).starts_with(snapshot_dir(sandbox, hf)) {
            bail!(
                "the pulled {field} {file} is not in hf.co/{}@{}",
                hf.repo,
                hf.revision
            );
        }
    }
    println!(
        "onnx: read back offline: ardana pull {} --tokenizer {tokenizer}",
        target.name
    );
    Ok(())
}

pub fn publish(sandbox: &Sandbox, name: &str, dry_run: bool) -> Result<()> {
    let root = sandbox.repo_root();
    let target = Target::load(root, name)?;
    let mut uploads = Vec::new();
    for repo in target.repos() {
        let local = local_snapshot(sandbox, repo)
            .with_context(|| format!("run `cargo xtask onnx convert {}` first", target.name))?;
        let snapshot = snapshot_dir(sandbox, &local);
        let card = std::fs::read_to_string(snapshot.join(CARD))
            .with_context(|| format!("reading the card of {repo}"))?;
        let folder = snapshot
            .strip_prefix(root)
            .unwrap_or(&snapshot)
            .display()
            .to_string();
        let message = format!(
            "{} from {}@{} (cargo xtask onnx convert)",
            target.name, target.source.repo, target.source.revision
        );
        let create = HfCommand::new(&[
            "repos",
            "create",
            repo,
            "--type",
            "model",
            "--public",
            "--exist-ok",
        ]);
        let upload = HfCommand::new(&[
            "upload",
            repo,
            &folder,
            ".",
            "--type",
            "model",
            "--commit-message",
            &message,
            "--format",
            "quiet",
        ]);
        let pin = Hf {
            repo: repo.to_string(),
            revision: "<the commit `hf upload` prints>".into(),
            files: local
                .files
                .iter()
                .filter(|file| !UNPINNED.contains(&file.as_str()))
                .cloned()
                .collect(),
        };
        println!(
            "publish: hf.co/{repo}\n\nThe model card it uploads ({CARD}):\n\n{card}\nThe commands, from the repository \
             root:\n\n  {create}\n  {upload}\n\nThe [[hf]] entry it pins in xtask/fetch.toml:\n\n{}\n",
            hf_entry(&pin)
        );
        uploads.push((create, upload, pin));
    }
    if dry_run {
        println!("publish: dry run, nothing uploaded");
        return Ok(());
    }
    let token = token(sandbox)?;
    for (create, upload, mut pin) in uploads {
        let failed = || {
            format!(
                "publishing hf.co/{} failed; the commands above publish it by hand",
                pin.repo
            )
        };
        create.run(sandbox, &token).with_context(failed)?;
        let url = upload.run(sandbox, &token).with_context(failed)?;
        pin.revision = url
            .trim()
            .rsplit_once("/commit/")
            .map(|(_, commit)| commit.to_string())
            .filter(|commit| commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()))
            .with_context(|| format!("`{upload}` printed no commit: {}", url.trim()))?;
        let path = root.join("xtask/fetch.toml");
        let text = std::fs::read_to_string(&path)?;
        std::fs::write(&path, pin_entry(&text, &pin))?;
        println!(
            "publish: pinned hf.co/{}@{} in xtask/fetch.toml",
            pin.repo, pin.revision
        );
    }
    Ok(())
}

/// The snapshot `convert` wrote for `repo`: the commit in `refs/main` and the files under it.
fn local_snapshot(sandbox: &Sandbox, repo: &str) -> Result<Hf> {
    let mut hf = Hf {
        repo: repo.to_string(),
        revision: String::new(),
        files: Vec::new(),
    };
    let refs = hf.repo_dir(sandbox).join("refs/main");
    hf.revision = std::fs::read_to_string(&refs)
        .with_context(|| format!("{} is missing", refs.display()))?
        .trim()
        .to_string();
    hf.files = files_in(&snapshot_dir(sandbox, &hf))?;
    Ok(hf)
}

/// The user's Hugging Face token (Q12): `HF_TOKEN`, else the real home's `~/.cache/huggingface/token` that
/// `hf auth login` writes. Children get it only as `HF_TOKEN`; nothing prints it.
fn token(sandbox: &Sandbox) -> Result<String> {
    if let Ok(token) = std::env::var("HF_TOKEN")
        && !token.trim().is_empty()
    {
        return Ok(token.trim().to_string());
    }
    let path = sandbox.real_home().join(".cache/huggingface/token");
    let token = std::fs::read_to_string(&path).unwrap_or_default();
    if token.trim().is_empty() {
        bail!(
            "no Hugging Face token: HF_TOKEN is unset and {} holds none; set HF_TOKEN, or run the commands above \
             by hand",
            path.display()
        );
    }
    Ok(token.trim().to_string())
}

/// One `hf` command of the venv, as `publish` prints it and runs it from the repo root.
struct HfCommand {
    args: Vec<String>,
}

impl HfCommand {
    fn new(args: &[&str]) -> HfCommand {
        HfCommand {
            args: args.iter().map(|arg| arg.to_string()).collect(),
        }
    }

    /// Runs the command with `token` in `HF_TOKEN` and returns its stdout; errors name the command, never the token.
    fn run(&self, sandbox: &Sandbox, token: &str) -> Result<String> {
        let root = sandbox.repo_root();
        let output = sandbox
            .command(root.join(VENV).join("bin/hf"))
            .current_dir(root)
            .args(&self.args)
            .env("HF_TOKEN", token)
            .stderr(Stdio::inherit())
            .output()
            .with_context(|| format!("running `{self}`"))?;
        if !output.status.success() {
            bail!("`{self}` failed ({})", output.status);
        }
        Ok(String::from_utf8(output.stdout)?)
    }
}

impl fmt::Display for HfCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{VENV}/bin/hf")?;
        for arg in &self.args {
            let plain = arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./:@=<>".contains(c));
            if plain {
                write!(f, " {arg}")?;
            } else {
                write!(f, " {}", quote(arg))?;
            }
        }
        Ok(())
    }
}

/// `hf` as an `xtask/fetch.toml` `[[hf]]` entry.
fn hf_entry(hf: &Hf) -> String {
    let files: Vec<String> = hf.files.iter().map(|file| format!("\"{file}\"")).collect();
    format!(
        "[[hf]]\nrepo = \"{}\"\nrevision = \"{}\"\nfiles = [{}]",
        hf.repo,
        hf.revision,
        files.join(", ")
    )
}

/// `fetch.toml` with `hf` as the `[[hf]]` entry of its repository: replacing the one there, else after the last
/// `[[hf]]` entry. An entry runs from its `[[hf]]` line to the next blank, comment or table line.
fn pin_entry(text: &str, hf: &Hf) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let entry: Vec<String> = hf_entry(hf).lines().map(str::to_string).collect();
    let end_of = |lines: &[String], start: usize| {
        (start + 1..lines.len())
            .find(|&i| {
                let line = lines[i].trim();
                line.is_empty() || line.starts_with('#') || line.starts_with('[')
            })
            .unwrap_or(lines.len())
    };
    let entries: Vec<(usize, usize)> = (0..lines.len())
        .filter(|&i| lines[i].trim() == "[[hf]]")
        .map(|start| (start, end_of(&lines, start)))
        .collect();
    let repo_line = format!("repo = \"{}\"", hf.repo);
    match entries.iter().find(|(start, end)| {
        lines[*start..*end]
            .iter()
            .any(|line| line.trim() == repo_line)
    }) {
        Some(&(start, end)) => {
            lines.splice(start..end, entry);
        }
        None => {
            let at = entries.last().map_or(lines.len(), |&(_, end)| end);
            lines.splice(at..at, std::iter::once(String::new()).chain(entry));
        }
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY_TEXT: &str = "# sizes are bytes\ndefault = \"a\"\n\n[[model]]\nname = \"a\"\nsize = 1\n\n\
                                [model.browser]\nweights = \"hf.co/ardana-ai/a-ONNX\"\nquant = \"int4\"\nsize = 2\n\n\
                                [[model]]\nname = \"b\"\nsize = 3\n\n[model.browser]\nquant = \"int8\"\n\n\
                                [[model]]\nname = \"c\"\nsize = 4\n";

    #[test]
    fn records_the_built_sizes() {
        let recorded = record_sizes(LIBRARY_TEXT, "a", 1_101_201_408, Some(811_843_552)).unwrap();
        assert_eq!(
            recorded,
            LIBRARY_TEXT
                .replace(
                    "name = \"a\"\nsize = 1\n",
                    "name = \"a\"\nsize = 811_843_552\n"
                )
                .replace(
                    "quant = \"int4\"\nsize = 2\n",
                    "quant = \"int4\"\nsize = 1_101_201_408\n"
                )
        );
        // Without a GGUF the model's own size stays; a browser table without a size gets one.
        let recorded = record_sizes(LIBRARY_TEXT, "b", 891_355_136, None).unwrap();
        assert_eq!(
            recorded,
            LIBRARY_TEXT.replace(
                "quant = \"int8\"\n\n[[model]]",
                "quant = \"int8\"\nsize = 891_355_136\n\n[[model]]"
            )
        );
        for (name, err) in [("c", "no [model.browser] table"), ("d", "has no line")] {
            let message = record_sizes(LIBRARY_TEXT, name, 1, None)
                .unwrap_err()
                .to_string();
            assert!(message.contains(err), "{message}");
        }
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1_000");
    }

    #[test]
    fn pins_hub_entries() {
        let fetch = "[[tool]]\nname = \"x\"\n\n[[hf]]\nrepo = \"o/a\"\nrevision = \"1\"\nfiles = [\"a\"]\n\n\
                     [[hf]]\nrepo = \"ardana-ai/b\"\nrevision = \"2\"\nfiles = [\"b\"]\n\n\
                     [[hf_local]]\nrepo = \"o/c\"\nrevision = \"3\"\nfiles = [\"c\"]\n";
        let pin = |repo: &str| Hf {
            repo: repo.into(),
            revision: "9".repeat(40),
            files: vec!["model.onnx".into(), "tokenizer.json".into()],
        };
        let entry = |repo: &str| {
            format!(
                "[[hf]]\nrepo = \"{repo}\"\nrevision = \"{}\"\nfiles = [\"model.onnx\", \"tokenizer.json\"]\n",
                "9".repeat(40)
            )
        };
        assert_eq!(
            pin_entry(fetch, &pin("ardana-ai/b")),
            fetch.replace(
                "[[hf]]\nrepo = \"ardana-ai/b\"\nrevision = \"2\"\nfiles = [\"b\"]\n",
                &entry("ardana-ai/b")
            )
        );
        assert_eq!(
            pin_entry(fetch, &pin("ardana-ai/new")),
            fetch.replace(
                "\n[[hf_local]]",
                &format!("\n{}\n[[hf_local]]", entry("ardana-ai/new"))
            )
        );
    }
}

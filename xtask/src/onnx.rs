//! `cargo xtask onnx convert|publish <name>`: a library model's browser variant, ONNX weights for onnxruntime-web, and
//! the GGUF ardana-ai hosts for it, both built from the checkpoint the library document (C1,
//! `../ardana-landing/src/lib/data/models.json`) names as the model's `source` (Q6, `docs/guidelines/onnx.md`); never
//! in CI.
//!
//! `convert` downloads the checkpoint into `tmp/hf`, exports the ONNX with `xtask/scripts/onnx_export.py` in the
//! `tmp/py/onnx` venv it creates, converts the GGUF with llama.cpp's `convert_hf_to_gguf.py` when the model's `weights`
//! is an ardana-ai repository, writes each repository into the sandbox Hub cache as the snapshot `publish` uploads,
//! records the commits, sizes and profile it built in the document and its snapshot (C5,
//! `crates/ardana-registry/tests/data/models.json`), which hold the same bytes (Q14), and reads both repositories back
//! offline with the release `ardana pull`, reading the document (`ARDANA_LIBRARY`). `publish` prints its `hf`
//! commands, the model cards, the `[[hf]]` entries and the document entry, then (without `--dry-run`) uploads the
//! snapshots to huggingface.co/ardana-ai with the user's token and pins them in `xtask/fetch.toml` and the document.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use ardana_registry::BrowserCache;
use ardana_registry::library::{Library, LibraryEntry};
use serde_json::Value;

use crate::env::quote;
use crate::fetch::{Hf, Manifest, blob_name, link_snapshot, remove_if_present};
use crate::python::pinned_venv;
use crate::sandbox::{SNAPSHOT, Sandbox, run};
use crate::serve::build_ardana;

/// The Hugging Face organisation that hosts what `convert` builds.
const ORG: &str = "ardana-ai";
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

/// The library document (C1) in the landing checkout beside this one, relative to the repo root: what `onnx` reads and
/// records into (Q14), formatted by the landing's Prettier (`../ardana-landing/prettier.config.js`).
const DOCUMENT: &str = "../ardana-landing/src/lib/data/models.json";
/// That Prettier's line width, and the columns it counts for a tab.
const PRINT_WIDTH: usize = 100;
const TAB_WIDTH: usize = 2;

/// The library document and its snapshot (C5), which `onnx` writes together so they hold the same bytes.
struct Documents {
    landing: PathBuf,
    snapshot: PathBuf,
}

impl Documents {
    /// The document beside the repo root `root`; an error naming [`DOCUMENT`] when that checkout is not there.
    fn find(root: &Path) -> Result<Documents> {
        let landing = root.join(DOCUMENT);
        if !landing.is_file() {
            bail!(
                "{DOCUMENT} is missing ({}): `cargo xtask onnx` records into the landing's library document, so \
                 check out ardana-landing beside this repository",
                landing.display()
            );
        }
        Ok(Documents {
            landing,
            snapshot: root.join(SNAPSHOT),
        })
    }

    fn read(&self) -> Result<String> {
        std::fs::read_to_string(&self.landing)
            .with_context(|| format!("reading {}", self.landing.display()))
    }

    /// Writes `text` as the document and as its snapshot.
    fn write(&self, text: &str) -> Result<()> {
        for path in [&self.landing, &self.snapshot] {
            std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
        }
        Ok(())
    }
}

/// What the library document says to build for one model.
#[derive(Debug)]
struct Target {
    /// The model's document entry.
    entry: LibraryEntry,
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
    /// The model `name` as the library document at `path` describes it, read as `ardana` reads it.
    fn load(path: &Path, name: &str) -> Result<Target> {
        let library = Library::from_file(path)
            .with_context(|| format!("reading the library document {}", path.display()))?;
        let Some(model) = library.find(name).map(|pick| pick.model) else {
            bail!(
                "no library model {name}; the library has {}",
                library.names().join(", ")
            );
        };
        let Some(browser) = &model.browser else {
            bail!(
                "library model {} has no browser variant: {DOCUMENT} gives it no browser table (weights, commit, \
                 quant, size, profile)",
                model.name
            );
        };
        let source = model.source.as_deref().with_context(|| {
            format!(
                "library model {} names no `source` checkpoint in {DOCUMENT}",
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
            entry: model.clone(),
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
    let documents = Documents::find(root)?;
    let target = Target::load(&documents.landing, name)?;
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

    let profile = browser_profile(sandbox, &target, &onnx)?;
    let recorded = record_build(
        &documents.read()?,
        &target.name,
        (&onnx, browser_size, profile),
        gguf.as_ref().map(|(hf, size)| (hf, *size)),
    )?;
    documents.write(&recorded)?;
    println!("onnx: recorded the build in {DOCUMENT} and {SNAPSHOT}");
    read_back(
        sandbox,
        &target,
        &documents.landing,
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

/// The profile a server sends for the browser variant of `target` that `convert` wrote as `onnx`
/// (`GET /v1/browser/<name>/profile`), read from the sandbox Hub cache as a server reads it.
fn browser_profile(sandbox: &Sandbox, target: &Target, onnx: &Hf) -> Result<Value> {
    let mut entry = target.entry.clone();
    if let Some(browser) = &mut entry.browser {
        browser.commit = onnx.revision.clone();
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .context("starting a runtime for the hub cache")?;
    let cache = BrowserCache::at(&sandbox.tmp().join("hf/hub"))?;
    let variant = runtime.block_on(cache.variant(&entry)).with_context(|| {
        format!(
            "reading the browser variant of {} from hf.co/{}@{}",
            target.name, onnx.repo, onnx.revision
        )
    })?;
    Ok(serde_json::to_value(&variant.profile)?)
}

/// `text`, a library document, with what `convert` built for the model `name`: its browser variant's commit, size and
/// profile, and, when it built the GGUF, the model's commit and size and the size of the tag of its quant.
fn record_build(
    text: &str,
    name: &str,
    (onnx, browser_size, profile): (&Hf, u64, Value),
    gguf: Option<(&Hf, u64)>,
) -> Result<String> {
    edit_entry(text, name, |entry| {
        *field(entry, &["browser", "commit"])? = onnx.revision.as_str().into();
        *field(entry, &["browser", "size"])? = browser_size.into();
        *field(entry, &["browser", "profile"])? = profile;
        if let Some((hf, size)) = gguf {
            *field(entry, &["commit"])? = hf.revision.as_str().into();
            *field(entry, &["size"])? = size.into();
            let tag = field(entry, &["quant"])?
                .as_str()
                .unwrap_or_default()
                .to_lowercase();
            let item = field(entry, &["tags"])?
                .as_array_mut()
                .and_then(|tags| tags.iter_mut().find(|item| item["tag"] == tag.as_str()))
                .with_context(|| format!("the {name} model has no tag {tag}"))?;
            *field(item, &["size"])? = size.into();
        }
        Ok(())
    })
}

/// `text`, a library document, with the entry of the model `name` changed by `change`. The document must be in the
/// formatting [`format_document`] writes, so every other byte stays as it was.
fn edit_entry(
    text: &str,
    name: &str,
    change: impl FnOnce(&mut Value) -> Result<()>,
) -> Result<String> {
    let mut document: Value = serde_json::from_str(text).context("parsing the library document")?;
    if format_document(&document) != text {
        bail!(
            "the library document is not formatted as `onnx` writes it (the landing's Prettier, every object one \
             key a line), so writing it would change more than the {name} entry"
        );
    }
    change(entry_of(&mut document, name)?)?;
    Ok(format_document(&document))
}

/// The entry of the model `name` in `document`.
fn entry_of<'a>(document: &'a mut Value, name: &str) -> Result<&'a mut Value> {
    document
        .get_mut("models")
        .and_then(Value::as_array_mut)
        .and_then(|models| models.iter_mut().find(|entry| entry["name"] == name))
        .with_context(|| format!("the library document has no model {name}"))
}

/// The field at `path` of `entry`, which must have it.
fn field<'a>(entry: &'a mut Value, path: &[&str]) -> Result<&'a mut Value> {
    path.iter().try_fold(entry, |value, key| {
        value
            .get_mut(*key)
            .with_context(|| format!("the entry has no {}", path.join(".")))
    })
}

/// `value` as the landing's Prettier formats the document: every object with keys one key a line (Prettier keeps an
/// object expanded as the document has it), an array on one line when it fits, else its numbers filling the lines
/// and anything else one item a line; a newline at the end.
fn format_document(value: &Value) -> String {
    let mut out = String::new();
    write_json(&mut out, value, 0, 0, 0);
    out.push('\n');
    out
}

/// Writes `value` nested `depth` deep, starting at `column`, with `trail` columns after it on its line (a comma).
fn write_json(out: &mut String, value: &Value, depth: usize, column: usize, trail: usize) {
    let indent = |out: &mut String, depth: usize| out.extend(std::iter::repeat_n('\t', depth));
    let inner = (depth + 1) * TAB_WIDTH;
    match value {
        Value::Object(map) if !map.is_empty() => {
            out.push_str("{\n");
            for (i, (key, item)) in map.iter().enumerate() {
                let comma = i + 1 < map.len();
                let key = format!("{}: ", Value::from(key.as_str()));
                indent(out, depth + 1);
                out.push_str(&key);
                write_json(
                    out,
                    item,
                    depth + 1,
                    inner + width(&key),
                    usize::from(comma),
                );
                if comma {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, depth);
            out.push('}');
        }
        Value::Array(items) => match flat(value) {
            Some(flat) if column + width(&flat) + trail <= PRINT_WIDTH => out.push_str(&flat),
            _ => {
                out.push_str("[\n");
                if items.iter().all(Value::is_number) {
                    indent(out, depth + 1);
                    let mut at = inner;
                    for (i, item) in items.iter().enumerate() {
                        let text = if i + 1 < items.len() {
                            format!("{item},")
                        } else {
                            item.to_string()
                        };
                        if i > 0 && at + 1 + width(&text) <= PRINT_WIDTH {
                            out.push(' ');
                            at += 1;
                        } else if i > 0 {
                            out.push('\n');
                            indent(out, depth + 1);
                            at = inner;
                        }
                        out.push_str(&text);
                        at += width(&text);
                    }
                    out.push('\n');
                } else {
                    for (i, item) in items.iter().enumerate() {
                        let comma = i + 1 < items.len();
                        indent(out, depth + 1);
                        write_json(out, item, depth + 1, inner, usize::from(comma));
                        if comma {
                            out.push(',');
                        }
                        out.push('\n');
                    }
                }
                indent(out, depth);
                out.push(']');
            }
        },
        _ => out.push_str(&value.to_string()),
    }
}

/// `value` on one line, as Prettier prints it where it fits; `None` for what it always breaks, an object with keys.
fn flat(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => map.is_empty().then(|| "{}".to_string()),
        Value::Array(items) => {
            let items: Option<Vec<String>> = items.iter().map(flat).collect();
            Some(format!("[{}]", items?.join(", ")))
        }
        _ => Some(value.to_string()),
    }
}

/// The columns Prettier counts for `text`, which holds no tab.
fn width(text: &str) -> usize {
    text.chars().count()
}

/// Reads both repositories back offline through `ardana_registry`'s Hub: the release `ardana pull`s the model into the
/// scratch `home` with the ONNX repository's tokenizer, reading the library `document`, which holds the commits just
/// written; its entry must point into the snapshots just written (the GGUF one when `convert` built it, else the
/// library's GGUF from `cargo xtask fetch`).
fn read_back(
    sandbox: &Sandbox,
    target: &Target,
    document: &Path,
    home: &Path,
    onnx: &Hf,
    gguf: Option<&Hf>,
) -> Result<()> {
    let ardana = build_ardana(sandbox)?;
    let tokenizer = format!("hf.co/{}", onnx.repo);
    run(sandbox
        .command(&ardana)
        .env("ARDANA_HOME", home)
        .env("ARDANA_LIBRARY", document)
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
    let documents = Documents::find(root)?;
    let target = Target::load(&documents.landing, name)?;
    let (uploads, plan) = plan(sandbox, &target, &documents.read()?)?;
    println!("{plan}");
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
        let document = pin_document(&documents.read()?, &target, &pin)?;
        std::fs::write(&path, pin_entry(&text, &pin))?;
        documents.write(&document)?;
        println!(
            "publish: pinned hf.co/{}@{} in xtask/fetch.toml, {DOCUMENT} and {SNAPSHOT}",
            pin.repo, pin.revision
        );
    }
    Ok(())
}

/// One repository `publish` uploads: the `hf repos create` and `hf upload` commands and the `[[hf]]` entry it pins.
type Upload = (HfCommand, HfCommand, Hf);

/// What `publish` uploads for `target` and what it prints first: each repository's card, commands and `[[hf]]` entry,
/// then the entry it pins in the library `document`, its commits those `hf upload` prints.
fn plan(sandbox: &Sandbox, target: &Target, document: &str) -> Result<(Vec<Upload>, String)> {
    let root = sandbox.repo_root();
    let mut uploads = Vec::new();
    let mut report = String::new();
    let mut pinned = document.to_string();
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
        report.push_str(&format!(
            "publish: hf.co/{repo}\n\nThe model card it uploads ({CARD}):\n\n{card}\nThe commands, from the repository \
             root:\n\n  {create}\n  {upload}\n\nThe [[hf]] entry it pins in xtask/fetch.toml:\n\n{}\n\n",
            hf_entry(&pin)
        ));
        pinned = pin_document(&pinned, target, &pin)?;
        uploads.push((create, upload, pin));
    }
    let mut pinned: Value = serde_json::from_str(&pinned)?;
    report.push_str(&format!(
        "The entry it pins in {DOCUMENT} and {SNAPSHOT}:\n\n{}",
        format_document(entry_of(&mut pinned, &target.name)?)
    ));
    Ok((uploads, report))
}

/// The library `document` with `hf`, which `publish` uploaded for `target`, as a commit of its entry: the browser
/// variant's for the ONNX repository, the model's own for the GGUF one.
fn pin_document(document: &str, target: &Target, hf: &Hf) -> Result<String> {
    let path: &[&str] = if hf.repo == target.onnx {
        &["browser", "commit"]
    } else {
        &["commit"]
    };
    edit_entry(document, &target.name, |entry| {
        *field(entry, path)? = hf.revision.as_str().into();
        Ok(())
    })
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

    fn repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
    }

    /// The library snapshot (C5), a copy of the landing's document, so in its formatting.
    fn snapshot_text() -> String {
        std::fs::read_to_string(repo_root().join(SNAPSHOT)).unwrap()
    }

    /// A model of the snapshot as `onnx` reads it.
    fn snapshot_target(name: &str) -> Target {
        Target::load(&repo_root().join(SNAPSHOT), name).unwrap()
    }

    /// A fresh directory under the test's temp dir (`tmp/sys` under cargo).
    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("onnx-{test}-{}", std::process::id()));
        remove_if_present(&dir).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn hf(repo: &str, revision: &str) -> Hf {
        Hf {
            repo: repo.into(),
            revision: revision.into(),
            files: Vec::new(),
        }
    }

    /// R5.1: recording a build changes only the entry's browser commit, size and profile and, with a GGUF, its commit,
    /// size and the size of its quant's tag; every other byte, and the landing's Prettier formatting, stay.
    #[test]
    fn records_the_build_in_the_document() {
        let text = snapshot_text();
        let document: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(format_document(&document), text, "the snapshot round-trips");

        let target = snapshot_target("decider-0.8b");
        let browser = target.entry.browser.as_ref().unwrap();
        let (onnx, gguf) = (
            hf(&target.onnx, &"c".repeat(40)),
            hf("ardana-ai/x", &"d".repeat(40)),
        );
        // A chat profile whose head fills more than one line, as Prettier fills an array of numbers.
        let head: Vec<u32> = (0..30).map(|i| 248_000 + i * 7).collect();
        let profile = serde_json::json!({
            "name": "decider-0.8b-v2",
            "layout": {"kind": "chat", "head": head, "tail": [248046, 198]},
            "temperature": 1.0,
            "temperature_by_type": {"choice": 1.5},
            "isolated_levels": false,
            "release_date": "2026-10-01"
        });
        let recorded =
            record_build(&text, "decider-0.8b", (&onnx, 5, profile), Some((&gguf, 7))).unwrap();
        let at = text.find("\"name\": \"decider-0.8b\"").unwrap();
        let start = at + text[at..].find("\"profile\": {").unwrap();
        let end = start + text[start..].find("\n\t\t\t\t}").unwrap() + 6;
        let size = target.entry.size.to_string();
        assert_eq!(
            text.matches(&size).count(),
            2,
            "the model's size and its tag's"
        );
        let expected = format!("{}{PROFILE}{}", &text[..start], &text[end..])
            .replace(&target.entry.commit, &gguf.revision)
            .replace(&browser.commit, &onnx.revision)
            .replace(&size, "7")
            .replace(&format!("\"size\": {},", browser.size), "\"size\": 5,");
        assert_eq!(recorded, expected);

        // Without a GGUF only the browser variant's fields change.
        let target = snapshot_target("qwen3.5-0.8b");
        let browser = target.entry.browser.as_ref().unwrap();
        let profile = browser.profile.clone();
        let recorded = record_build(&text, "qwen3.5-0.8b", (&onnx, 5, profile), None).unwrap();
        let expected = text
            .replace(&browser.commit, &onnx.revision)
            .replace(&format!("\"size\": {},", browser.size), "\"size\": 5,");
        assert_eq!(recorded, expected);

        let err = record_build(&text, "decider-4b", (&onnx, 5, Value::Null), None).unwrap_err();
        assert_eq!(err.to_string(), "the entry has no browser.commit");
        let err = record_build(&text, "nope", (&onnx, 5, Value::Null), None).unwrap_err();
        assert_eq!(err.to_string(), "the library document has no model nope");
        // A document in another formatting is refused rather than rewritten.
        let collapsed = text.replacen(
            "{\n\t\t\t\t\t\t\"kind\": \"plain\"\n\t\t\t\t\t}",
            "{ \"kind\": \"plain\" }",
            1,
        );
        assert_ne!(collapsed, text);
        let err =
            record_build(&collapsed, "qwen3.5-0.8b", (&onnx, 5, Value::Null), None).unwrap_err();
        assert!(
            err.to_string()
                .contains("not formatted as `onnx` writes it"),
            "{err}"
        );
    }

    /// The profile `records_the_build_in_the_document` records, as the landing's Prettier 3.9.9 formats it at its depth
    /// (`npx prettier --check` of the recorded document passes): the head filling lines of at most 100 columns.
    const PROFILE: &str = "\"profile\": {\n\t\t\t\t\t\"name\": \"decider-0.8b-v2\",\n\t\t\t\t\t\"layout\": {\n\
                           \t\t\t\t\t\t\"kind\": \"chat\",\n\t\t\t\t\t\t\"head\": [\n\
                           \t\t\t\t\t\t\t248000, 248007, 248014, 248021, 248028, 248035, 248042, 248049, 248056, 248063,\n\
                           \t\t\t\t\t\t\t248070, 248077, 248084, 248091, 248098, 248105, 248112, 248119, 248126, 248133,\n\
                           \t\t\t\t\t\t\t248140, 248147, 248154, 248161, 248168, 248175, 248182, 248189, 248196, 248203\n\
                           \t\t\t\t\t\t],\n\t\t\t\t\t\t\"tail\": [248046, 198]\n\t\t\t\t\t},\n\
                           \t\t\t\t\t\"temperature\": 1.0,\n\t\t\t\t\t\"temperature_by_type\": {\n\
                           \t\t\t\t\t\t\"choice\": 1.5\n\t\t\t\t\t},\n\t\t\t\t\t\"isolated_levels\": false,\n\
                           \t\t\t\t\t\"release_date\": \"2026-10-01\"\n\t\t\t\t}";

    /// R5.2: without the landing checkout beside the repository, `convert` and `publish` stop before anything else,
    /// naming the document.
    #[test]
    fn needs_the_landing_checkout() {
        let base = scratch("needs-landing");
        let root = base.join("ardana");
        let sandbox = Sandbox::new(&root, &base).unwrap();
        for result in [
            convert(&sandbox, "decider-0.8b"),
            publish(&sandbox, "decider-0.8b", true),
        ] {
            let err = format!("{:#}", result.unwrap_err());
            assert!(
                err.starts_with("../ardana-landing/src/lib/data/models.json is missing"),
                "{err}"
            );
        }
        assert!(!root.join("tmp/onnx").exists(), "convert built nothing");
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// R5.4: `publish --dry-run` prints each repository's `[[hf]]` entry and then the document entry it pins, its
    /// commits those `hf upload` prints, and writes no file.
    #[test]
    fn a_dry_run_prints_the_entry_and_writes_nothing() {
        let base = scratch("dry-run");
        let root = base.join("ardana");
        let landing = base.join("ardana-landing/src/lib/data/models.json");
        let copies = [
            (repo_root().join(SNAPSHOT), root.join(SNAPSHOT)),
            (repo_root().join(SNAPSHOT), landing.clone()),
            (
                repo_root().join("xtask/fetch.toml"),
                root.join("xtask/fetch.toml"),
            ),
        ];
        for (from, to) in &copies {
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(from, to).unwrap();
        }
        let sandbox = Sandbox::new(&root, &base).unwrap();
        let target = snapshot_target("decider-0.8b");
        // The local snapshots `convert` would have written.
        for repo in target.repos() {
            let snapshot = snapshot_dir(&sandbox, &hf(repo, "local"));
            std::fs::create_dir_all(&snapshot).unwrap();
            for file in [CARD, "LICENSE", "model.onnx", TOKENIZER] {
                std::fs::write(snapshot.join(file), file).unwrap();
            }
            let refs = hf(repo, "local").repo_dir(&sandbox).join("refs");
            std::fs::create_dir_all(&refs).unwrap();
            std::fs::write(refs.join("main"), "local").unwrap();
        }
        let before: Vec<Vec<u8>> = copies
            .iter()
            .map(|(_, to)| std::fs::read(to).unwrap())
            .collect();

        publish(&sandbox, "decider-0.8b", true).unwrap();
        let after: Vec<Vec<u8>> = copies
            .iter()
            .map(|(_, to)| std::fs::read(to).unwrap())
            .collect();
        assert!(before == after, "the dry run wrote a file");

        let (uploads, report) = plan(&sandbox, &target, &snapshot_text()).unwrap();
        let repos: Vec<&str> = uploads
            .iter()
            .map(|(_, _, pin)| pin.repo.as_str())
            .collect();
        assert_eq!(repos, target.repos());
        for (_, _, pin) in &uploads {
            assert!(report.contains(&hf_entry(pin)), "{report}");
            assert_eq!(pin.files, ["model.onnx", TOKENIZER]);
        }
        let placeholder = "<the commit `hf upload` prints>";
        let entry = report
            .split_once(&format!(
                "The entry it pins in {DOCUMENT} and {SNAPSHOT}:\n\n"
            ))
            .map(|(_, entry)| entry)
            .unwrap_or_else(|| panic!("{report}"));
        let mut document: Value = serde_json::from_str(&snapshot_text()).unwrap();
        let expected = entry_of(&mut document, "decider-0.8b").unwrap();
        expected["commit"] = placeholder.into();
        expected["browser"]["commit"] = placeholder.into();
        assert_eq!(serde_json::from_str::<Value>(entry).unwrap(), *expected);
        assert!(
            entry.contains(&format!("\n\t\"commit\": \"{placeholder}\",\n")),
            "{entry}"
        );
        assert!(
            entry.contains(&format!("\n\t\t\"commit\": \"{placeholder}\",\n")),
            "{entry}"
        );
        std::fs::remove_dir_all(&base).unwrap();
    }

    /// R5.5: what `publish` pins after its uploads, in `xtask/fetch.toml` and the document, agrees: the browser
    /// variant's commit is its ONNX repository's revision, and the model's own its GGUF repository's when ardana-ai
    /// hosts it; nothing else in the document changes.
    #[test]
    fn publish_pins_the_document() {
        let text = snapshot_text();
        let fetch = std::fs::read_to_string(repo_root().join("xtask/fetch.toml")).unwrap();
        for name in ["decider-0.8b", "qwen3.5-0.8b"] {
            let target = snapshot_target(name);
            let (mut pinned_fetch, mut pinned) = (fetch.clone(), text.clone());
            let mut expected = text.clone();
            for (repo, revision) in target.repos().into_iter().zip(["1", "2"]) {
                let pin = Hf {
                    files: vec!["model.onnx".into()],
                    ..hf(repo, &revision.repeat(40))
                };
                pinned_fetch = pin_entry(&pinned_fetch, &pin);
                pinned = pin_document(&pinned, &target, &pin).unwrap();
                let old = match &target.gguf {
                    Some((gguf, _)) if gguf == repo => &target.entry.commit,
                    _ => &target.entry.browser.as_ref().unwrap().commit,
                };
                expected = expected.replace(old, &pin.revision);
            }
            assert_eq!(pinned, expected);
            let manifest: Manifest = toml::from_str(&pinned_fetch).unwrap();
            let revision = |repo: &str| {
                manifest
                    .hf
                    .iter()
                    .find(|hf| hf.repo == repo)
                    .map(|hf| hf.revision.clone())
                    .unwrap()
            };
            let mut document: Value = serde_json::from_str(&pinned).unwrap();
            let entry = entry_of(&mut document, name).unwrap();
            assert_eq!(entry["browser"]["commit"], revision(&target.onnx));
            match &target.gguf {
                Some((gguf, _)) => assert_eq!(entry["commit"], revision(gguf)),
                None => assert_eq!(entry["commit"], target.entry.commit.as_str()),
            }
        }
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

//! `cargo xtask fetch [--check]`: installs, or verifies, every entry of
//! `xtask/fetch.toml` inside `tmp/`: `[[tool]]` entries, `[[hf]]` model
//! repositories and `[[hf_local]]` copies from the user's own Hub cache.
//! `cargo xtask fetch --tests` installs only the Hub files plain `cargo test`
//! reads ([`fetch_tests`]).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::python::{MIN_PYTHON, managed_venv, uv};
use crate::sandbox::Sandbox;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default)]
    pub tool: Vec<Tool>,
    #[serde(default)]
    pub hf: Vec<Hf>,
    /// Gated repositories (Q18): copied read-only from the real home's Hub cache.
    #[serde(default)]
    pub hf_local: Vec<Hf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub name: String,
    /// A literal version, or `cargo-lock:<crate>` for the version `Cargo.lock` resolves.
    pub version: String,
    pub source: Source,
    /// `github`: the release asset, a `.tar.gz` with a `.sha256` next to it. `uv-venv`: a requirements source
    /// inside `tmp/` (a clone's `pyproject.toml`) installed instead of `<name>==<version>`. `local-copy`: the file
    /// to copy, relative to the real home.
    pub url: Option<String>,
    /// `github`: the executable inside the archive, linked into `tmp/bin`. `uv-venv`: the venv inside `tmp/`.
    /// `npm`: the package directory, relative to the repo root. `local-copy`: the copy inside `tmp/`.
    pub path: Option<String>,
    /// `git`: the full commit checked out into `tmp/src/<name>`.
    pub rev: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// `cargo install` through [`Sandbox::cargo_install`].
    Cargo,
    /// A GitHub release archive, checked against its `.sha256`.
    Github,
    /// A git repository at `url`, checked out at `rev` into `tmp/src/<name>`.
    Git,
    /// `uv tool install <name>==<version>` into `UV_TOOL_DIR`, its executables in `tmp/bin`.
    UvTool,
    /// A uv venv at `tmp/<path>` holding `<name>==<version>`, or the requirements `url` names.
    UvVenv,
    /// `npm ci` in the package directory `path`, whose `package.json` pins `<name>` at exactly `version`.
    Npm,
    /// A file of the real home (`url`) copied, read only, to `tmp/<path>`.
    LocalCopy,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Source::Cargo => "cargo",
            Source::Github => "github",
            Source::Git => "git",
            Source::UvTool => "uv-tool",
            Source::UvVenv => "uv-venv",
            Source::Npm => "npm",
            Source::LocalCopy => "local-copy",
        }
    }
}

impl Manifest {
    pub fn load(repo_root: &Path) -> Result<Manifest> {
        let path = repo_root.join("xtask/fetch.toml");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// Installs every entry that does not pass its check yet.
pub fn fetch(sandbox: &Sandbox) -> Result<()> {
    let manifest = Manifest::load(sandbox.repo_root())?;
    for tool in &manifest.tool {
        let version = tool.resolved_version(sandbox.repo_root())?;
        if tool.check(sandbox, &version).is_ok() {
            continue;
        }
        println!("fetch: installing {}", tool.label(&version));
        tool.install(sandbox, &version)?;
        tool.check(sandbox, &version)?;
    }
    for repo in &manifest.hf {
        if repo.check(sandbox).is_ok() {
            continue;
        }
        println!("fetch: downloading {}@{}", repo.repo, repo.revision);
        repo.install(sandbox)?;
        repo.check(sandbox)?;
    }
    for repo in &manifest.hf_local {
        if repo.check(sandbox).is_ok() {
            continue;
        }
        println!(
            "fetch: copying {}@{} from {}",
            repo.repo,
            repo.revision,
            repo.local_dir(sandbox).display()
        );
        repo.install_local(sandbox)?;
        repo.check(sandbox)?;
    }
    Ok(())
}

/// Installs only what plain `cargo test` reads: every `[[hf]]` and
/// `[[hf_local]]` file but the GGUF weights. A gated `[[hf_local]]` repository
/// is copied from this machine's Hub cache when it holds the revision, else
/// downloaded from the Hub, which needs `HF_TOKEN` with access to it (CI).
pub fn fetch_tests(sandbox: &Sandbox) -> Result<()> {
    let manifest = Manifest::load(sandbox.repo_root())?;
    let repos = manifest.hf.iter().map(|repo| (repo, false));
    let gated = manifest.hf_local.iter().map(|repo| (repo, true));
    for (repo, gated) in repos.chain(gated) {
        let repo = repo.without_weights();
        if repo.check(sandbox).is_ok() {
            continue;
        }
        let local = repo
            .local_dir(sandbox)
            .join("snapshots")
            .join(&repo.revision);
        if gated && local.is_dir() {
            println!(
                "fetch: copying {}@{} from {}",
                repo.repo,
                repo.revision,
                local.display()
            );
            repo.install_local(sandbox)?;
        } else {
            println!("fetch: downloading {}@{}", repo.repo, repo.revision);
            repo.install(sandbox).with_context(|| {
                if gated {
                    format!(
                        "{} is gated: set HF_TOKEN to a token with access to it",
                        repo.repo
                    )
                } else {
                    format!("downloading {}", repo.repo)
                }
            })?;
        }
        repo.check(sandbox)?;
    }
    Ok(())
}

/// Fails naming every entry that is missing or differs from `fetch.toml`.
pub fn check(sandbox: &Sandbox) -> Result<()> {
    let manifest = Manifest::load(sandbox.repo_root())?;
    let mut missing = Vec::new();
    for tool in &manifest.tool {
        let verdict = tool
            .resolved_version(sandbox.repo_root())
            .and_then(|version| tool.check(sandbox, &version).map(|()| version));
        if let Err(err) = verdict {
            missing.push(format!(
                "tool {} ({}): {err:#}",
                tool.name,
                tool.source.name()
            ));
        }
    }
    for repo in &manifest.hf {
        if let Err(err) = repo.check(sandbox) {
            missing.push(format!("hf {}@{}: {err:#}", repo.repo, repo.revision));
        }
    }
    for repo in &manifest.hf_local {
        if let Err(err) = repo.check(sandbox) {
            missing.push(format!("hf_local {}@{}: {err:#}", repo.repo, repo.revision));
        }
    }
    if !missing.is_empty() {
        bail!(
            "{} fetch.toml entr{} missing; run `cargo xtask fetch`:\n  {}",
            missing.len(),
            if missing.len() == 1 {
                "y is"
            } else {
                "ies are"
            },
            missing.join("\n  ")
        );
    }
    Ok(())
}

impl Tool {
    fn label(&self, version: &str) -> String {
        format!("{} {version} ({})", self.name, self.source.name())
    }

    pub fn resolved_version(&self, repo_root: &Path) -> Result<String> {
        match self.version.strip_prefix("cargo-lock:") {
            Some(krate) => lockfile_version(repo_root, krate),
            None => Ok(self.version.clone()),
        }
    }

    fn install(&self, sandbox: &Sandbox, version: &str) -> Result<()> {
        match self.source {
            Source::Cargo => sandbox.cargo_install(&self.name, version),
            Source::Github => self.install_github(sandbox, version),
            Source::Git => self.install_git(sandbox),
            Source::UvTool => {
                let pin = format!("{}=={version}", self.name);
                let args = [
                    "tool", "install", "--quiet", "--force", "--python", MIN_PYTHON,
                ];
                run_stdout(uv(sandbox).args(args).arg(pin)).map(drop)
            }
            Source::UvVenv => self.install_venv(sandbox, version),
            Source::Npm => {
                let dir = self.npm_dir(sandbox)?;
                run_stdout(sandbox.command("npm").current_dir(&dir).args([
                    "ci",
                    "--no-audit",
                    "--no-fund",
                ]))
                .with_context(|| format!("npm ci in {}", dir.display()))
                .map(drop)
            }
            Source::LocalCopy => {
                let (source, copy) = self.local_copy_paths(sandbox)?;
                std::fs::create_dir_all(copy.parent().context("the copy has no parent")?)?;
                std::fs::copy(&source, &copy).with_context(|| {
                    format!("copying {} to {}", source.display(), copy.display())
                })?;
                Ok(())
            }
        }
    }

    fn check(&self, sandbox: &Sandbox, version: &str) -> Result<()> {
        match self.source {
            Source::Cargo => check_cargo(sandbox, &self.name, version),
            Source::Github => {
                let (link, target) = self.github_paths(sandbox, version)?;
                let resolved = std::fs::read_link(&link)
                    .with_context(|| format!("{} is not a link", link.display()))?;
                if resolved != target || !target.is_file() {
                    bail!("{} does not point at {}", link.display(), target.display());
                }
                Ok(())
            }
            Source::Git => {
                let dir = self.git_dir(sandbox);
                let head = run_stdout(
                    sandbox
                        .command("git")
                        .arg("-C")
                        .arg(&dir)
                        .args(["rev-parse", "HEAD"]),
                )
                .with_context(|| format!("{} is not a git checkout", dir.display()))?;
                let rev = self.rev()?;
                if head.trim() != rev {
                    bail!("{} is at {}, not {rev}", dir.display(), head.trim());
                }
                Ok(())
            }
            Source::UvTool => self.check_uv_tool(sandbox, version),
            Source::UvVenv => self.check_venv(sandbox, version),
            Source::Npm => self.check_npm(sandbox, version),
            Source::LocalCopy => self.check_local_copy(sandbox),
        }
    }

    /// The npm package directory `path`, inside the repo.
    fn npm_dir(&self, sandbox: &Sandbox) -> Result<PathBuf> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| anyhow!("tool {} needs `path`", self.name))?;
        Ok(sandbox.repo_root().join(path))
    }

    /// `package.json` pins `<name>` at exactly `version` and `node_modules` holds that version.
    fn check_npm(&self, sandbox: &Sandbox, version: &str) -> Result<()> {
        let dir = self.npm_dir(sandbox)?;
        let read = |path: PathBuf| -> Result<serde_json::Value> {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("{} is missing", path.display()))?;
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
        };
        let manifest = read(dir.join("package.json"))?;
        let pinned = ["dependencies", "devDependencies"]
            .iter()
            .find_map(|section| manifest[section][&self.name].as_str());
        if pinned != Some(version) {
            bail!(
                "{} pins {} at {pinned:?}, not exactly {version}",
                dir.join("package.json").display(),
                self.name
            );
        }
        let installed = read(
            dir.join("node_modules")
                .join(&self.name)
                .join("package.json"),
        )?;
        if installed["version"].as_str() != Some(version) {
            bail!(
                "{} has {} {}, not {version}",
                dir.join("node_modules").display(),
                self.name,
                installed["version"]
            );
        }
        Ok(())
    }

    /// The real-home source and the `tmp/` copy of a `local-copy` entry.
    fn local_copy_paths(&self, sandbox: &Sandbox) -> Result<(PathBuf, PathBuf)> {
        let source = sandbox.real_home().join(self.url()?);
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| anyhow!("tool {} needs `path`", self.name))?;
        Ok((source, sandbox.tmp().join(path)))
    }

    /// The copy is an executable file, byte for byte the source while the source still exists.
    fn check_local_copy(&self, sandbox: &Sandbox) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let (source, copy) = self.local_copy_paths(sandbox)?;
        let meta =
            std::fs::metadata(&copy).with_context(|| format!("{} is missing", copy.display()))?;
        if !meta.is_file() || meta.len() == 0 || meta.permissions().mode() & 0o111 == 0 {
            bail!("{} is not a non-empty executable file", copy.display());
        }
        if source.exists() && std::fs::read(&source)? != std::fs::read(&copy)? {
            bail!("{} differs from {}", copy.display(), source.display());
        }
        Ok(())
    }

    /// `uv tool list` shows the tool at `version`, its env runs a uv-managed Python and its executable is in
    /// `tmp/bin`.
    fn check_uv_tool(&self, sandbox: &Sandbox, version: &str) -> Result<()> {
        let listed = run_stdout(uv(sandbox).args(["tool", "list"]))?;
        let wanted = format!("{} v{version}", self.name);
        if !listed.lines().any(|line| line.trim() == wanted) {
            bail!("uv tool list does not show {wanted}");
        }
        let env = sandbox.tmp().join("uv/tools").join(&self.name);
        if !managed_venv(sandbox, &env) {
            bail!("{} does not run a uv-managed Python", env.display());
        }
        let exe = sandbox.bin_dir().join(&self.name);
        if !exe.is_file() {
            bail!("{} is missing", exe.display());
        }
        Ok(())
    }

    /// `tmp/<path>`, where a `uv-venv` entry's venv lives.
    fn venv_dir(&self, sandbox: &Sandbox) -> Result<PathBuf> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| anyhow!("tool {} needs `path`", self.name))?;
        Ok(sandbox.tmp().join(path))
    }

    /// The requirements source a `uv-venv` entry installs from, when it names one.
    fn requirements(&self, sandbox: &Sandbox) -> Option<PathBuf> {
        self.url.as_deref().map(|file| sandbox.tmp().join(file))
    }

    /// Creates the venv with a uv-managed Python unless it already runs one, then installs into it.
    fn install_venv(&self, sandbox: &Sandbox, version: &str) -> Result<()> {
        let venv = self.venv_dir(sandbox)?;
        if !managed_venv(sandbox, &venv) {
            run_stdout(
                uv(sandbox)
                    .args(["venv", "--quiet", "--clear", "--python", MIN_PYTHON])
                    .arg(&venv),
            )?;
        }
        let mut install = uv(sandbox);
        install
            .args(["pip", "install", "--quiet", "--python"])
            .arg(venv.join("bin/python"));
        match self.requirements(sandbox) {
            Some(file) => install.arg("-r").arg(file),
            None => install.arg(format!("{}=={version}", self.name)),
        };
        run_stdout(&mut install).map(drop)
    }

    /// The venv runs a uv-managed Python of at least [`MIN_PYTHON`] and holds `<name>` at `version`, or satisfies
    /// its requirements without installing anything.
    fn check_venv(&self, sandbox: &Sandbox, version: &str) -> Result<()> {
        let venv = self.venv_dir(sandbox)?;
        if !managed_venv(sandbox, &venv) {
            bail!("{} is not a venv on a uv-managed Python", venv.display());
        }
        let python = venv.join("bin/python");
        let (major, minor) = MIN_PYTHON
            .split_once('.')
            .context("MIN_PYTHON is <major>.<minor>")?;
        let recent = run_stdout(sandbox.command(&python).arg("-c").arg(format!(
            "import sys; print(sys.version_info >= ({major}, {minor}))"
        )))?;
        if recent.trim() != "True" {
            bail!("{} is older than Python {MIN_PYTHON}", python.display());
        }
        match self.requirements(sandbox) {
            Some(file) => {
                let output = uv(sandbox)
                    .args(["pip", "install", "--dry-run", "--offline", "--python"])
                    .arg(&python)
                    .arg("-r")
                    .arg(&file)
                    .output()
                    .context("running uv pip install --dry-run")?;
                let said = String::from_utf8_lossy(&output.stderr)
                    + String::from_utf8_lossy(&output.stdout);
                if !output.status.success() || !said.contains("Would make no changes") {
                    bail!(
                        "{} does not satisfy {}: {}",
                        venv.display(),
                        file.display(),
                        said.trim()
                    );
                }
            }
            None => {
                let installed = run_stdout(sandbox.command(&python).arg("-c").arg(format!(
                    "import importlib.metadata as m; print(m.version({:?}))",
                    self.name
                )))
                .with_context(|| format!("{} has no {}", venv.display(), self.name))?;
                if installed.trim() != version {
                    bail!(
                        "{} has {} {}, not {version}",
                        venv.display(),
                        self.name,
                        installed.trim()
                    );
                }
            }
        }
        Ok(())
    }

    fn rev(&self) -> Result<&str> {
        let rev = self
            .rev
            .as_deref()
            .ok_or_else(|| anyhow!("tool {} needs `rev`", self.name))?;
        if rev.len() != 40 || !rev.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("tool {}: `rev` must be a full 40-hex commit id", self.name);
        }
        Ok(rev)
    }

    /// `tmp/src/<name>`, where research clones live.
    fn git_dir(&self, sandbox: &Sandbox) -> PathBuf {
        sandbox.tmp().join("src").join(&self.name)
    }

    /// Fetches exactly `rev` (shallow) into a fresh `tmp/src/<name>` and
    /// checks it out detached.
    fn install_git(&self, sandbox: &Sandbox) -> Result<()> {
        let url = self.url()?;
        let rev = self.rev()?;
        let dir = self.git_dir(sandbox);
        remove_if_present(&dir)?;
        std::fs::create_dir_all(&dir)?;
        let git = |args: &[&str]| {
            run_stdout(sandbox.command("git").arg("-C").arg(&dir).args(args))
                .with_context(|| format!("git {} in {}", args.join(" "), dir.display()))
        };
        git(&["init", "--quiet"])?;
        git(&["fetch", "--quiet", "--depth", "1", url, rev])?;
        git(&[
            "-c",
            "advice.detachedHead=false",
            "checkout",
            "--quiet",
            "FETCH_HEAD",
        ])?;
        Ok(())
    }

    fn url(&self) -> Result<&str> {
        self.url
            .as_deref()
            .ok_or_else(|| anyhow!("tool {} needs `url`", self.name))
    }

    /// The `tmp/bin` link and the extracted executable it points at.
    fn github_paths(&self, sandbox: &Sandbox, version: &str) -> Result<(PathBuf, PathBuf)> {
        let inner = self
            .path
            .as_deref()
            .ok_or_else(|| anyhow!("tool {} needs `path`", self.name))?;
        let file_name = Path::new(inner)
            .file_name()
            .ok_or_else(|| anyhow!("tool {} has an empty `path`", self.name))?;
        let target = self.extract_dir(sandbox, version).join(inner);
        Ok((sandbox.bin_dir().join(file_name), target))
    }

    fn extract_dir(&self, sandbox: &Sandbox, version: &str) -> PathBuf {
        sandbox.tmp().join("tools").join(&self.name).join(version)
    }

    fn install_github(&self, sandbox: &Sandbox, version: &str) -> Result<()> {
        let url = self.url()?;
        let asset = url
            .rsplit('/')
            .next()
            .filter(|name| name.ends_with(".tar.gz"));
        let asset =
            asset.ok_or_else(|| anyhow!("tool {}: `url` must name a .tar.gz", self.name))?;
        let downloads = sandbox.tmp().join("downloads");
        std::fs::create_dir_all(&downloads)?;
        let archive = downloads.join(asset);
        let digest_file = downloads.join(format!("{asset}.sha256"));
        download(sandbox, url, &archive)?;
        download(sandbox, &format!("{url}.sha256"), &digest_file)?;

        let expected = std::fs::read_to_string(&digest_file)?;
        let expected = expected.split_whitespace().next().unwrap_or_default();
        let actual = run_stdout(sandbox.command("shasum").args(["-a", "256"]).arg(&archive))?;
        let actual = actual.split_whitespace().next().unwrap_or_default();
        if expected.is_empty() || actual != expected {
            bail!("{asset}: sha256 {actual} does not match the published {expected}");
        }

        let dir = self.extract_dir(sandbox, version);
        let staging = dir.with_extension("partial");
        remove_if_present(&staging)?;
        remove_if_present(&dir)?;
        std::fs::create_dir_all(&staging)?;
        run_stdout(
            sandbox
                .command("tar")
                .arg("-xzf")
                .arg(&archive)
                .arg("-C")
                .arg(&staging),
        )?;
        std::fs::rename(&staging, &dir)?;

        let (link, target) = self.github_paths(sandbox, version)?;
        std::fs::create_dir_all(sandbox.bin_dir())?;
        remove_if_present(&link)?;
        std::os::unix::fs::symlink(&target, &link)
            .with_context(|| format!("linking {}", link.display()))?;
        Ok(())
    }
}

/// A Hugging Face model repository pinned at a commit; `files` are downloaded
/// (`[[hf]]`) or copied from the real home's Hub cache (`[[hf_local]]`) into
/// `tmp/hf/hub` in the official cache layout.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hf {
    /// `org/name`.
    pub repo: String,
    /// The full commit id the files are taken from; also written to `refs/main`.
    pub revision: String,
    pub files: Vec<String>,
}

/// One file of a repo revision as the Hub API lists it.
struct HubFile {
    /// The blob name in the cache: the LFS sha256, else the git blob id.
    blob: String,
    lfs: bool,
    size: u64,
}

impl Hf {
    /// This repository without its GGUF weights: the files plain `cargo test` reads.
    pub fn without_weights(&self) -> Hf {
        Hf {
            repo: self.repo.clone(),
            revision: self.revision.clone(),
            files: self
                .files
                .iter()
                .filter(|file| !file.ends_with(".gguf"))
                .cloned()
                .collect(),
        }
    }

    fn cache_name(&self) -> String {
        format!("models--{}", self.repo.replace('/', "--"))
    }

    /// `tmp/hf/hub/models--<org>--<name>`.
    pub fn repo_dir(&self, sandbox: &Sandbox) -> PathBuf {
        sandbox.tmp().join("hf/hub").join(self.cache_name())
    }

    /// The repo in the real home's Hub cache, the source of `[[hf_local]]`.
    pub fn local_dir(&self, sandbox: &Sandbox) -> PathBuf {
        sandbox
            .real_home()
            .join(".cache/huggingface/hub")
            .join(self.cache_name())
    }

    fn validate(&self) -> Result<()> {
        if self.repo.split('/').count() != 2 {
            bail!("hf repo {} must be `org/name`", self.repo);
        }
        if self.revision.len() != 40 || !self.revision.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!(
                "hf {}: `revision` must be a full 40-hex commit id",
                self.repo
            );
        }
        Ok(())
    }

    /// Passes when `refs/main` holds the revision and every file resolves
    /// through `snapshots/<revision>/` to a non-empty blob.
    fn check(&self, sandbox: &Sandbox) -> Result<()> {
        self.validate()?;
        let dir = self.repo_dir(sandbox);
        let refs = dir.join("refs/main");
        let main = std::fs::read_to_string(&refs)
            .with_context(|| format!("{} is missing", refs.display()))?;
        if main.trim() != self.revision {
            bail!(
                "{} holds {}, not {}",
                refs.display(),
                main.trim(),
                self.revision
            );
        }
        let snapshot = dir.join("snapshots").join(&self.revision);
        for file in &self.files {
            let path = snapshot.join(file);
            let meta = std::fs::metadata(&path)
                .with_context(|| format!("{} is missing", path.display()))?;
            if !meta.is_file() || meta.len() == 0 {
                bail!("{} is not a non-empty file", path.display());
            }
        }
        Ok(())
    }

    /// Downloads every file of the pinned revision into `blobs/`, verifying
    /// the LFS sha256 or the git blob id, links it from `snapshots/` and writes
    /// `refs/main`, the layout `huggingface_hub` and `hf-hub` read.
    fn install(&self, sandbox: &Sandbox) -> Result<()> {
        self.validate()?;
        let listing = self.listing(sandbox)?;
        let dir = self.repo_dir(sandbox);
        let blobs = dir.join("blobs");
        let snapshot = dir.join("snapshots").join(&self.revision);
        std::fs::create_dir_all(&blobs)?;
        for file in &self.files {
            let meta = listing
                .iter()
                .find(|(name, _)| name == file)
                .map(|(_, meta)| meta)
                .ok_or_else(|| anyhow!("{}@{} has no file {file}", self.repo, self.revision))?;
            let blob = blobs.join(&meta.blob);
            let complete = std::fs::metadata(&blob).is_ok_and(|m| m.len() == meta.size);
            if !complete {
                let partial = blobs.join(format!("{}.incomplete", meta.blob));
                let url = format!(
                    "https://huggingface.co/{}/resolve/{}/{file}",
                    self.repo, self.revision
                );
                hub_download(sandbox, &url, &partial)?;
                verify_blob(sandbox, &partial, meta)
                    .with_context(|| format!("verifying {file} of {}", self.repo))?;
                std::fs::rename(&partial, &blob)?;
            }
            link_snapshot(&snapshot, file, &meta.blob)?;
        }
        std::fs::create_dir_all(dir.join("refs"))?;
        std::fs::write(dir.join("refs/main"), &self.revision)?;
        Ok(())
    }

    /// Copies every file of the pinned snapshot from the real home's Hub
    /// cache, reading it only: the blob named by the snapshot link is copied
    /// into `blobs/`, verified against that name (a git blob id or an LFS
    /// sha256), linked from `snapshots/` and `refs/main` is written.
    fn install_local(&self, sandbox: &Sandbox) -> Result<()> {
        self.validate()?;
        let source = self
            .local_dir(sandbox)
            .join("snapshots")
            .join(&self.revision);
        let dir = self.repo_dir(sandbox);
        let blobs = dir.join("blobs");
        let snapshot = dir.join("snapshots").join(&self.revision);
        std::fs::create_dir_all(&blobs)?;
        for file in &self.files {
            let link = source.join(file);
            let resolved = std::fs::canonicalize(&link).with_context(|| {
                format!(
                    "{} is missing; the Hub cache of this machine must hold {}@{}",
                    link.display(),
                    self.repo,
                    self.revision
                )
            })?;
            let blob = resolved
                .file_name()
                .and_then(|name| name.to_str())
                .with_context(|| format!("{} has no blob name", resolved.display()))?
                .to_string();
            let meta = HubFile {
                lfs: blob.len() == 64,
                size: std::fs::metadata(&resolved)?.len(),
                blob,
            };
            let partial = blobs.join(format!("{}.incomplete", meta.blob));
            std::fs::copy(&resolved, &partial)
                .with_context(|| format!("copying {}", resolved.display()))?;
            verify_blob(sandbox, &partial, &meta)
                .with_context(|| format!("verifying {file} of {}", self.repo))?;
            std::fs::rename(&partial, blobs.join(&meta.blob))?;
            link_snapshot(&snapshot, file, &meta.blob)?;
        }
        std::fs::create_dir_all(dir.join("refs"))?;
        std::fs::write(dir.join("refs/main"), &self.revision)?;
        Ok(())
    }

    /// `(file name, blob)` for every file of the revision, from the Hub API.
    fn listing(&self, sandbox: &Sandbox) -> Result<Vec<(String, HubFile)>> {
        let url = format!(
            "https://huggingface.co/api/models/{}/revision/{}?blobs=true",
            self.repo, self.revision
        );
        let mut curl = sandbox.command("curl");
        curl.args(["-fsSL", "--retry", "3"]).arg(&url);
        let text = run_hub(&mut curl).with_context(|| format!("listing {url}"))?;
        let info: serde_json::Value = serde_json::from_str(&text)?;
        if info["sha"].as_str() != Some(self.revision.as_str()) {
            bail!("the Hub resolved {} to {}", self.revision, info["sha"]);
        }
        let siblings = info["siblings"]
            .as_array()
            .context("no `siblings` in the Hub listing")?;
        let mut files = Vec::new();
        for sibling in siblings {
            let name = sibling["rfilename"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let lfs = sibling["lfs"]["sha256"].as_str();
            let blob = lfs
                .or_else(|| sibling["blobId"].as_str())
                .with_context(|| format!("no blob id for {name}"))?;
            let size = sibling["size"]
                .as_u64()
                .with_context(|| format!("no size for {name}"))?;
            let meta = HubFile {
                blob: blob.to_string(),
                lfs: lfs.is_some(),
                size,
            };
            files.push((name, meta));
        }
        Ok(files)
    }
}

/// Links `snapshots/<rev>/<file>` relatively to `blobs/<blob>`.
fn link_snapshot(snapshot: &Path, file: &str, blob: &str) -> Result<()> {
    let link = snapshot.join(file);
    let parent = link.parent().context("snapshot link has no parent")?;
    std::fs::create_dir_all(parent)?;
    let depth = Path::new(file).components().count() + 1;
    let target = Path::new(&"../".repeat(depth)).join("blobs").join(blob);
    remove_if_present(&link)?;
    std::os::unix::fs::symlink(&target, &link)
        .with_context(|| format!("linking {}", link.display()))
}

/// An LFS file must hash to its sha256, any other file to its git blob id.
fn verify_blob(sandbox: &Sandbox, path: &Path, meta: &HubFile) -> Result<()> {
    let actual = if meta.lfs {
        run_stdout(sandbox.command("shasum").args(["-a", "256"]).arg(path))?
    } else {
        run_stdout(
            sandbox
                .command("git")
                .args(["hash-object", "--no-filters"])
                .arg(path),
        )?
    };
    let actual = actual.split_whitespace().next().unwrap_or_default();
    if actual != meta.blob {
        bail!(
            "{} hashes to {actual}, expected {}",
            path.display(),
            meta.blob
        );
    }
    Ok(())
}

/// Passes when `tmp/.crates2.json` records `krate` at `version` and all its
/// binaries exist in `tmp/bin`.
fn check_cargo(sandbox: &Sandbox, krate: &str, version: &str) -> Result<()> {
    let path = sandbox.tmp().join(".crates2.json");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("{} is missing", path.display()))?;
    let crates: serde_json::Value = serde_json::from_str(&text)?;
    let prefix = format!("{krate} {version} ");
    let install = crates["installs"]
        .as_object()
        .and_then(|installs| installs.iter().find(|(key, _)| key.starts_with(&prefix)))
        .map(|(_, install)| install)
        .ok_or_else(|| {
            anyhow!(
                "{krate} {version} is not installed in {}",
                sandbox.tmp().display()
            )
        })?;
    for bin in install["bins"].as_array().into_iter().flatten() {
        let bin = sandbox.bin_dir().join(bin.as_str().unwrap_or_default());
        if !bin.is_file() {
            bail!("{} is missing", bin.display());
        }
    }
    Ok(())
}

/// The single version of `krate` in the workspace `Cargo.lock`.
pub fn lockfile_version(repo_root: &Path, krate: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct Lock {
        package: Vec<Package>,
    }
    #[derive(Deserialize)]
    struct Package {
        name: String,
        version: String,
    }
    let path = repo_root.join("Cargo.lock");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let lock: Lock =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let versions: Vec<String> = lock
        .package
        .into_iter()
        .filter(|p| p.name == krate)
        .map(|p| p.version)
        .collect();
    match versions.as_slice() {
        [version] => Ok(version.clone()),
        [] => bail!("Cargo.lock has no {krate}"),
        _ => bail!(
            "Cargo.lock has several {krate} versions: {}",
            versions.join(", ")
        ),
    }
}

fn download(sandbox: &Sandbox, url: &str, dest: &Path) -> Result<()> {
    run_stdout(
        sandbox
            .command("curl")
            .args(["-fsSL", "--retry", "3", "-o"])
            .arg(dest)
            .arg(url),
    )
    .with_context(|| format!("downloading {url}"))?;
    Ok(())
}

/// Downloads a Hub file like [`download`], authenticated by `HF_TOKEN` when it is set.
fn hub_download(sandbox: &Sandbox, url: &str, dest: &Path) -> Result<()> {
    let mut curl = sandbox.command("curl");
    curl.args(["-fsSL", "--retry", "3", "-o"])
        .arg(dest)
        .arg(url);
    run_hub(&mut curl).with_context(|| format!("downloading {url}"))?;
    Ok(())
}

/// Runs a curl call to huggingface.co. With `HF_TOKEN` set, the token reaches
/// curl as a bearer header through its config on stdin, never in its arguments.
fn run_hub(curl: &mut Command) -> Result<String> {
    let Ok(token) = std::env::var("HF_TOKEN") else {
        return run_stdout(curl);
    };
    let mut child = curl
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {:?}", curl.get_program()))?;
    let mut stdin = child.stdin.take().context("curl has no stdin")?;
    writeln!(stdin, "header = \"Authorization: Bearer {token}\"")?;
    drop(stdin);
    stdout_of(curl, child.wait_with_output()?)
}

fn run_stdout(cmd: &mut Command) -> Result<String> {
    let output = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    stdout_of(cmd, output)
}

fn stdout_of(cmd: &Command, output: Output) -> Result<String> {
    if !output.status.success() {
        bail!(
            "{:?} failed ({}): {}",
            cmd.get_program(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn remove_if_present(path: &Path) -> Result<()> {
    let result = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => Err(err),
    };
    result.with_context(|| format!("removing {}", path.display()))
}

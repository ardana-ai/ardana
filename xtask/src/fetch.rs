//! `cargo xtask fetch [--check]`: installs, or verifies, every entry of
//! `xtask/fetch.toml` inside `tmp/`: `[[tool]]` entries and `[[hf]]` model
//! repositories.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::sandbox::Sandbox;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default)]
    pub tool: Vec<Tool>,
    #[serde(default)]
    pub hf: Vec<Hf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub name: String,
    /// A literal version, or `cargo-lock:<crate>` for the version `Cargo.lock` resolves.
    pub version: String,
    pub source: Source,
    /// `github`: the release asset, a `.tar.gz` with a `.sha256` next to it.
    pub url: Option<String>,
    /// `github`: the executable inside the archive, linked into `tmp/bin`.
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
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Source::Cargo => "cargo",
            Source::Github => "github",
            Source::Git => "git",
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
        }
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
/// into `tmp/hf/hub` in the official cache layout.
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
    /// `tmp/hf/hub/models--<org>--<name>`.
    pub fn repo_dir(&self, sandbox: &Sandbox) -> PathBuf {
        sandbox
            .tmp()
            .join("hf/hub")
            .join(format!("models--{}", self.repo.replace('/', "--")))
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
                download(sandbox, &url, &partial)?;
                verify_blob(sandbox, &partial, meta)
                    .with_context(|| format!("verifying {file} of {}", self.repo))?;
                std::fs::rename(&partial, &blob)?;
            }
            let link = snapshot.join(file);
            let parent = link.parent().context("snapshot link has no parent")?;
            std::fs::create_dir_all(parent)?;
            let depth = Path::new(file).components().count() + 1;
            let target = Path::new(&"../".repeat(depth))
                .join("blobs")
                .join(&meta.blob);
            remove_if_present(&link)?;
            std::os::unix::fs::symlink(&target, &link)
                .with_context(|| format!("linking {}", link.display()))?;
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
        let text = run_stdout(
            sandbox
                .command("curl")
                .args(["-fsSL", "--retry", "3"])
                .arg(&url),
        )
        .with_context(|| format!("listing {url}"))?;
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

fn run_stdout(cmd: &mut Command) -> Result<String> {
    let output = cmd.output().with_context(|| format!("running {cmd:?}"))?;
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

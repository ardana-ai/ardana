//! `cargo xtask fetch [--check]`: installs, or verifies, every entry of
//! `xtask/fetch.toml` inside `tmp/`.

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
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// `cargo install` through [`Sandbox::cargo_install`].
    Cargo,
    /// A GitHub release archive, checked against its `.sha256`.
    Github,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Source::Cargo => "cargo",
            Source::Github => "github",
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
        }
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

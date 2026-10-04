//! The project-local sandbox (Q21): every tool xtask runs keeps its home, caches,
//! downloads and temp files under the repo's gitignored `tmp/`, and
//! [`Sandbox::guarded`] proves a step wrote nothing under the real home.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::guard::HomeSnapshot;

/// Variables pointing into `tmp/`, as paths relative to it. `.cargo/config.toml`
/// `[env]` repeats `HF_HOME`, `ARDANA_HOME`, `ARDANA_TMP` and `TMPDIR`.
const TMP_VARS: &[(&str, &str)] = &[
    ("HOME", "home"),
    ("HF_HOME", "hf"),
    ("ARDANA_HOME", "ardana"),
    ("ARDANA_TMP", "ardana-tmp"),
    ("TMPDIR", "sys"),
    ("XDG_CACHE_HOME", "cache"),
    ("UV_CACHE_DIR", "cache/uv"),
    ("UV_PYTHON_INSTALL_DIR", "uv/python"),
    ("UV_TOOL_DIR", "uv/tools"),
    ("UV_TOOL_BIN_DIR", "bin"),
    ("UV_PYTHON_BIN_DIR", "bin"),
    ("PIP_CACHE_DIR", "cache/pip"),
    ("npm_config_cache", "cache/npm"),
    ("PLAYWRIGHT_BROWSERS_PATH", "ms-playwright"),
    ("IMPECCABLE_HOME", "impeccable"),
    ("IMPECCABLE_CACHE_ROOT", "cache/impeccable"),
];

/// The impeccable binary the launcher resolves first, relative to `tmp/`.
const IMPECCABLE_BIN: &str = "impeccable/bin/0.1.5/impeccable";

/// The library snapshot (C5), relative to the repo root: the document every process the sandbox runs reads the
/// model library from (`ARDANA_LIBRARY`, as `.cargo/config.toml` hands it to cargo-run processes), and what
/// `build-playground` and `onnx` read where they read the library.
pub const SNAPSHOT: &str = "crates/ardana-registry/tests/data/models.json";

/// Switches that stop impeccable from writing check files under `~/.impeccable`.
const IMPECCABLE_FLAGS: &[&str] = &[
    "IMPECCABLE_NO_UPDATE_CHECK",
    "IMPECCABLE_NO_STALENESS_CHECK",
    "IMPECCABLE_NO_TELEMETRY",
];

/// Toolchain and model-store locations that stay in the real home: `HOME`
/// moves into `tmp/`, so these are pinned explicitly.
const REAL_HOME_VARS: &[(&str, &str)] = &[
    ("RUSTUP_HOME", ".rustup"),
    ("CARGO_HOME", ".cargo"),
    ("OLLAMA_MODELS", ".ollama/models"),
];

#[derive(Debug)]
pub struct Sandbox {
    repo_root: PathBuf,
    tmp: PathBuf,
    real_home: PathBuf,
}

impl Sandbox {
    /// Creates `tmp/` and every directory the environment points at.
    pub fn new(repo_root: &Path, real_home: &Path) -> Result<Sandbox> {
        if !repo_root.is_absolute() || !real_home.is_absolute() {
            bail!(
                "sandbox paths must be absolute: repo root {}, home {}",
                repo_root.display(),
                real_home.display()
            );
        }
        let tmp = repo_root.join("tmp");
        for (_, rel) in TMP_VARS {
            let dir = tmp.join(rel);
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        Ok(Sandbox {
            repo_root: repo_root.to_path_buf(),
            tmp,
            real_home: real_home.to_path_buf(),
        })
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// The gitignored `tmp/` directory every tool writes into.
    pub fn tmp(&self) -> &Path {
        &self.tmp
    }

    /// The user's real home, which the guard watches.
    pub fn real_home(&self) -> &Path {
        &self.real_home
    }

    /// `tmp/bin`, where fetched tools live; first on every command's `PATH`.
    pub fn bin_dir(&self) -> PathBuf {
        self.tmp.join("bin")
    }

    /// The library snapshot ([`SNAPSHOT`]), absolute.
    pub fn snapshot(&self) -> PathBuf {
        self.repo_root.join(SNAPSHOT)
    }

    /// The Q21 tool environment, absolute paths only: every cache under `tmp/`, and the library read from the
    /// snapshot, never the network.
    pub fn env(&self) -> Vec<(String, OsString)> {
        let mut env: Vec<(String, OsString)> = TMP_VARS
            .iter()
            .map(|(name, rel)| (name.to_string(), self.tmp.join(rel).into_os_string()))
            .collect();
        env.push(("ARDANA_LIBRARY".into(), self.snapshot().into_os_string()));
        env.push((
            "IMPECCABLE_BIN".into(),
            self.tmp.join(IMPECCABLE_BIN).into_os_string(),
        ));
        env.extend(
            IMPECCABLE_FLAGS
                .iter()
                .map(|name| (name.to_string(), "1".into())),
        );
        env.extend(
            REAL_HOME_VARS
                .iter()
                .map(|(name, rel)| (name.to_string(), self.real_home.join(rel).into_os_string())),
        );
        env
    }

    /// A command running `program` in the sandbox environment. A bare program
    /// name is looked up on the sandbox `PATH`, where `tmp/bin` comes first.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut path = self.bin_dir().into_os_string();
        if let Some(inherited) = std::env::var_os("PATH") {
            path.push(":");
            path.push(inherited);
        }
        let mut cmd = Command::new(program);
        cmd.envs(self.env()).env("PATH", path);
        cmd
    }

    /// Installs `krate` at exactly `version` into `tmp/bin` with its own cargo
    /// home, so neither `~/.cargo/bin` nor the user's crate list changes.
    pub fn cargo_install(&self, krate: &str, version: &str) -> Result<()> {
        let cargo_home = self.tmp.join("cargo");
        let status = self
            .command(cargo())
            .current_dir(&self.repo_root)
            .env("CARGO_HOME", &cargo_home)
            .args(["install", krate, "--version", version, "--locked", "--root"])
            .arg(&self.tmp)
            .arg("--target-dir")
            .arg(cargo_home.join("target"))
            .status()
            .with_context(|| format!("running cargo install {krate}"))?;
        if !status.success() {
            bail!("cargo install {krate} {version} failed ({status})");
        }
        Ok(())
    }

    /// Runs `f` and fails when it changed anything under the guarded paths of
    /// the real home, listing each change.
    pub fn guarded<T>(&self, step: &str, f: impl FnOnce(&Sandbox) -> Result<T>) -> Result<T> {
        let before = HomeSnapshot::take(&self.real_home)?;
        let result = f(self);
        let after = HomeSnapshot::take(&self.real_home)?;
        let changes = before.changes(&after);
        if changes.is_empty() {
            return result;
        }
        let mut report = format!(
            "home guard: step `{step}` changed {} path(s) under {}:",
            changes.len(),
            self.real_home.display()
        );
        for change in &changes {
            report.push_str(&format!("\n  {change}"));
        }
        match result {
            Ok(_) => bail!(report),
            Err(err) => Err(err.context(report)),
        }
    }
}

/// The cargo that runs xtask, else the one on `PATH`.
pub fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// Runs `cmd` with its output on the terminal; fails unless it exits 0.
pub fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("running {cmd:?}"))?;
    if !status.success() {
        bail!("{:?} failed ({status})", cmd.get_program());
    }
    Ok(())
}

/// Runs `cmd` and returns its stdout; fails with its stderr unless it exits 0.
pub fn run_stdout(cmd: &mut Command) -> Result<String> {
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

//! uv and uv-managed Python for the eval harnesses and the steps' own venvs (`python-tooling.md`): every interpreter
//! comes from uv into `UV_PYTHON_INSTALL_DIR`, never from the system.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

use crate::sandbox::{Sandbox, run};

/// The oldest Python jevcompat, `typesafe-sdk` and JevBench run on.
pub const MIN_PYTHON: &str = "3.10";

/// `uv` in the sandbox, restricted to uv-managed interpreters.
pub fn uv(sandbox: &Sandbox) -> Command {
    let mut cmd = sandbox.command("uv");
    cmd.env("UV_PYTHON_PREFERENCE", "only-managed");
    cmd
}

/// A step's own venv: created at `venv` on the uv-managed `python` unless it already runs a uv-managed interpreter,
/// then given exactly `packages` (`<name>==<version>` pins) from PyPI and, when `extra_index_url` names one, that
/// index too (PyTorch's CPU wheels); returns its interpreter.
pub fn pinned_venv(
    sandbox: &Sandbox,
    venv: &Path,
    python: &str,
    packages: &[&str],
    extra_index_url: Option<&str>,
) -> Result<PathBuf> {
    if !managed_venv(sandbox, venv) {
        run(uv(sandbox)
            .args(["venv", "--quiet", "--clear", "--python", python])
            .arg(venv))?;
    }
    let interpreter = venv.join("bin/python");
    let mut install = uv(sandbox);
    install
        .args(["pip", "install", "--quiet", "--python"])
        .arg(&interpreter);
    if let Some(url) = extra_index_url {
        install.args(["--extra-index-url", url]);
    }
    run(install.args(packages))?;
    Ok(interpreter)
}

/// Whether `venv` exists and runs a uv-managed interpreter from `UV_PYTHON_INSTALL_DIR`, never the system's.
pub fn managed_venv(sandbox: &Sandbox, venv: &Path) -> bool {
    let managed_dir = sandbox.tmp().join("uv/python");
    std::fs::read_to_string(venv.join("pyvenv.cfg")).is_ok_and(|cfg| {
        cfg.lines()
            .filter_map(|line| line.strip_prefix("home = "))
            .any(|home| Path::new(home.trim()).starts_with(&managed_dir))
    })
}

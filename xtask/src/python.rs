//! uv and uv-managed Python for the eval harnesses (`python-tooling.md`): every interpreter comes from uv into
//! `UV_PYTHON_INSTALL_DIR`, never from the system.

use std::path::Path;
use std::process::Command;

use crate::sandbox::Sandbox;

/// The oldest Python jevcompat, `typesafe-sdk` and JevBench run on.
pub const MIN_PYTHON: &str = "3.10";

/// `uv` in the sandbox, restricted to uv-managed interpreters.
pub fn uv(sandbox: &Sandbox) -> Command {
    let mut cmd = sandbox.command("uv");
    cmd.env("UV_PYTHON_PREFERENCE", "only-managed");
    cmd
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

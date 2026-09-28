use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use xtask::sandbox::Sandbox;
use xtask::{deps, docs, e2e, env, export, fetch, playground};

const USAGE: &str = "usage: cargo xtask <step>
  env [--claude]   print the sandbox environment as shell exports, or write it
                   into .claude/settings.local.json
  fetch [--check]  install, or verify, the xtask/fetch.toml entries into tmp/
  build            build the playground dist with trunk
  check-deps       check the workspace members and dependency direction
  check-docs       check CLAUDE.md, AGENTS.md and docs/guidelines
  export-decider   export decider's prompt layout cases into crates/ardana-core/tests/data/decider
  e2e <suite>      run an end-to-end suite";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask has no parent directory")?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let sandbox = || Sandbox::new(root, &real_home(root)?);
    match args.as_slice() {
        ["env"] => print!("{}", env::shell_exports(&sandbox()?)?),
        ["env", "--claude"] => {
            let path = root.join(".claude/settings.local.json");
            env::write_claude_settings(&sandbox()?, &path)?;
            println!("wrote the sandbox env into {}", path.display());
        }
        ["fetch"] => sandbox()?.guarded("fetch", fetch::fetch)?,
        ["fetch", "--check"] => fetch::check(&sandbox()?)?,
        ["build"] => sandbox()?.guarded("build", playground::build)?,
        ["check-deps"] => deps::check_deps(&deps::metadata(root)?)?,
        ["check-docs"] => docs::check_docs(root)?,
        ["export-decider"] => sandbox()?.guarded("export-decider", export::export_decider)?,
        ["e2e", suite] => sandbox()?.guarded(&format!("e2e {suite}"), |s| e2e::run(s, suite))?,
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

/// The user's home from `HOME`. A shell that already evaluated
/// `cargo xtask env` has `HOME` inside the sandbox, which would move the
/// guard and the pinned toolchain paths; that is refused.
fn real_home(root: &Path) -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
    if home.starts_with(root.join("tmp")) {
        bail!(
            "HOME is {}, inside the sandbox; run cargo xtask from a shell that has not evaluated `cargo xtask env`",
            home.display()
        );
    }
    Ok(home)
}

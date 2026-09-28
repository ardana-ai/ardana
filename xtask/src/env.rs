//! `cargo xtask env [--claude]`: the sandbox environment for shells and for
//! Claude Code's Bash tool (Q26).

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

use crate::sandbox::Sandbox;

/// `export` lines for `eval "$(cargo xtask env)"`, with `tmp/bin` put in
/// front of the shell's `PATH`.
pub fn shell_exports(sandbox: &Sandbox) -> Result<String> {
    let mut out = String::new();
    for (name, value) in sandbox.env() {
        out.push_str(&format!("export {name}={}\n", quote(utf8(&name, &value)?)));
    }
    let bin = sandbox.bin_dir();
    out.push_str(&format!(
        "export PATH={}:\"$PATH\"\n",
        quote(utf8("PATH", bin.as_os_str())?)
    ));
    Ok(out)
}

/// Merges every variable except `HOME` into the `env` map of the Claude Code
/// settings file at `path`, keeping all other keys.
pub fn write_claude_settings(sandbox: &Sandbox, path: &Path) -> Result<()> {
    let mut settings = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Value>(&text)
            .with_context(|| format!("parsing {}", path.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Value::Object(Map::new()),
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };
    let Some(root) = settings.as_object_mut() else {
        bail!("{} is not a JSON object", path.display());
    };
    let env = root
        .entry("env")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(env) = env.as_object_mut() else {
        bail!("{} has an `env` that is not an object", path.display());
    };
    for (name, value) in sandbox.env() {
        if name != "HOME" {
            env.insert(name.clone(), Value::String(utf8(&name, &value)?.to_owned()));
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(&settings)? + "\n";
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

fn utf8<'a>(name: &str, value: &'a std::ffi::OsStr) -> Result<&'a str> {
    value
        .to_str()
        .with_context(|| format!("{name} is not valid UTF-8: {value:?}"))
}

/// Single-quotes `value` for POSIX shells.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

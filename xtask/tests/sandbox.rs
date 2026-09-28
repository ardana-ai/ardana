use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use xtask::env::{shell_exports, write_claude_settings};
use xtask::sandbox::Sandbox;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn sandbox() -> (Sandbox, PathBuf) {
    let home = repo_root().join("tmp/fakehome");
    (Sandbox::new(repo_root(), &home).unwrap(), home)
}

fn env_map(sandbox: &Sandbox) -> BTreeMap<String, OsString> {
    sandbox.env().into_iter().collect()
}

/// `.cargo/config.toml` `[env]` reaches plain `cargo test` processes with the
/// same paths the sandbox gives tools, whatever the shell sets.
#[test]
fn cargo_env_points_into_tmp() {
    let tmp = repo_root().join("tmp");
    let (sandbox, _) = sandbox();
    let env = env_map(&sandbox);
    for name in ["HF_HOME", "ARDANA_HOME", "ARDANA_TMP", "TMPDIR"] {
        let value = PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} unset")));
        assert!(value.starts_with(&tmp), "{name}={}", value.display());
        assert_eq!(
            value.as_os_str(),
            env[name],
            "{name} differs from Sandbox::env"
        );
    }
    assert_eq!(std::env::temp_dir(), tmp.join("sys"));
}

#[test]
fn env_points_into_tmp() {
    let tmp = repo_root().join("tmp");
    let (sandbox, home) = sandbox();
    let env = env_map(&sandbox);
    for name in [
        "HOME",
        "HF_HOME",
        "ARDANA_HOME",
        "ARDANA_TMP",
        "TMPDIR",
        "XDG_CACHE_HOME",
        "UV_CACHE_DIR",
        "UV_PYTHON_INSTALL_DIR",
        "UV_TOOL_DIR",
        "UV_TOOL_BIN_DIR",
        "UV_PYTHON_BIN_DIR",
        "PIP_CACHE_DIR",
        "npm_config_cache",
        "PLAYWRIGHT_BROWSERS_PATH",
        "IMPECCABLE_HOME",
        "IMPECCABLE_CACHE_ROOT",
    ] {
        let value = Path::new(&env[name]);
        assert!(
            value.starts_with(&tmp) && value.is_dir(),
            "{name}={}",
            value.display()
        );
    }
    assert_eq!(env["HOME"], tmp.join("home").into_os_string());
    assert_eq!(
        env["IMPECCABLE_BIN"],
        tmp.join("impeccable/bin/0.1.5/impeccable").into_os_string()
    );
    for name in [
        "IMPECCABLE_NO_UPDATE_CHECK",
        "IMPECCABLE_NO_STALENESS_CHECK",
        "IMPECCABLE_NO_TELEMETRY",
    ] {
        assert_eq!(env[name], "1", "{name}");
    }
    assert_eq!(env["RUSTUP_HOME"], home.join(".rustup").into_os_string());
    assert_eq!(env["CARGO_HOME"], home.join(".cargo").into_os_string());
    assert_eq!(
        env["OLLAMA_MODELS"],
        home.join(".ollama/models").into_os_string()
    );

    let cmd = sandbox.command("true");
    let path = cmd
        .get_envs()
        .find(|(k, _)| *k == "PATH")
        .and_then(|(_, v)| v)
        .unwrap();
    let first = std::env::split_paths(path).next().unwrap();
    assert_eq!(first, tmp.join("bin"));

    let exports = shell_exports(&sandbox).unwrap();
    for name in env.keys() {
        assert!(
            exports.contains(&format!("export {name}='")),
            "{name} not exported"
        );
    }
    assert!(exports.contains(&format!(
        "export PATH='{}':\"$PATH\"",
        tmp.join("bin").display()
    )));

    let settings = tmp.join("test-settings/settings.local.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(
        &settings,
        json!({"permissions": {"allow": ["Bash(ls)"]}, "env": {"KEEP": "me", "HF_HOME": "/elsewhere"}})
            .to_string(),
    )
    .unwrap();
    write_claude_settings(&sandbox, &settings).unwrap();
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(written["permissions"]["allow"][0], "Bash(ls)");
    assert_eq!(written["env"]["KEEP"], "me");
    assert!(written["env"].get("HOME").is_none());
    for (name, value) in &env {
        if name != "HOME" {
            assert_eq!(written["env"][name], value.to_str().unwrap(), "{name}");
        }
    }
}

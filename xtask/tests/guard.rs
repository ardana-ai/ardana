use std::fs;
use std::path::{Path, PathBuf};

use xtask::sandbox::Sandbox;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn write(path: PathBuf, text: &str) -> anyhow::Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(path, text)?;
    Ok(())
}

#[test]
fn detects_home_write() {
    let home = repo_root().join("tmp/fakehome");
    if home.exists() {
        fs::remove_dir_all(&home).unwrap();
    }
    write(home.join(".cache/huggingface/hub/version.txt"), "1").unwrap();
    write(home.join(".cargo/.crates.toml"), "[v1]\n").unwrap();
    write(
        home.join(".cargo/registry/index/x"),
        "outside the guarded paths",
    )
    .unwrap();
    fs::create_dir_all(home.join(".npm")).unwrap();
    fs::create_dir_all(home.join(".impeccable")).unwrap();
    let sandbox = Sandbox::new(repo_root(), &home).unwrap();

    // Writes inside the project, outside the guarded paths and into the
    // excluded npm and impeccable files of existing directories pass.
    sandbox
        .guarded("allowed", |s| {
            write(s.tmp().join("sys/guard-probe"), "x")?;
            write(home.join(".cargo/registry/index/y"), "x")?;
            write(home.join(".npm/_cacache/index"), "x")?;
            write(home.join(".npm/_logs/debug.log"), "x")?;
            write(home.join(".impeccable/update-check.json"), "{}")?;
            Ok(())
        })
        .unwrap();

    let err = sandbox
        .guarded("write", |_| {
            write(home.join(".cache/huggingface/hub/models--x/blob"), "x")
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("step `write`"), "{err}");
    assert!(
        err.contains(&format!(
            "added    {}",
            home.join(".cache/huggingface/hub/models--x/blob").display()
        )),
        "{err}"
    );

    let err = sandbox
        .guarded("modify", |_| {
            write(home.join(".cargo/.crates.toml"), "[v1]\ntrunk = []\n")
        })
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(&format!(
            "modified {}",
            home.join(".cargo/.crates.toml").display()
        )),
        "{err}"
    );

    let err = sandbox
        .guarded("install", |_| write(home.join(".cargo/bin/trunk"), "x"))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(&format!(
            "added    {}",
            home.join(".cargo/bin/trunk").display()
        )),
        "{err}"
    );

    let err = sandbox
        .guarded("remove", |_| {
            Ok(fs::remove_file(
                home.join(".cache/huggingface/hub/version.txt"),
            )?)
        })
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(&format!(
            "removed  {}",
            home.join(".cache/huggingface/hub/version.txt").display()
        )),
        "{err}"
    );

    // A failing step that also wrote reports both.
    let err = sandbox
        .guarded("failing", |_| -> anyhow::Result<()> {
            write(home.join(".ollama/models/blobs/sha256-x"), "x")?;
            anyhow::bail!("step failed")
        })
        .unwrap_err();
    let report = format!("{err:#}");
    assert!(
        report.contains("step `failing`") && report.contains("step failed"),
        "{report}"
    );
}

//! Embeds the playground's `dist/` (built by `cargo xtask build`, never here: a build script running trunk would
//! deadlock on cargo's lock, rust-lang/cargo#8938). `ARDANA_PLAYGROUND_DIST` points at another directory; without an
//! `index.html` there the build warns and embeds `placeholder/` instead.

use std::path::PathBuf;

const DIST_VAR: &str = "ARDANA_PLAYGROUND_DIST";

fn main() {
    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    );
    let dist = std::env::var_os(DIST_VAR)
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("../ardana-playground/dist"));
    println!("cargo::rerun-if-env-changed={DIST_VAR}");
    println!("cargo::rerun-if-changed={}", dist.display());

    let dir = if dist.join("index.html").is_file() {
        dist
    } else {
        println!(
            "cargo::warning=no playground at {} (run `cargo xtask build`); serving a placeholder at /",
            dist.display()
        );
        let placeholder = manifest_dir.join("placeholder");
        println!("cargo::rerun-if-changed={}", placeholder.display());
        placeholder
    };
    memory_serve::load_directory(dir);
}

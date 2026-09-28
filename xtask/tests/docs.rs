use std::fs;
use std::path::{Path, PathBuf};

use xtask::docs::{GUIDELINES, check_docs};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

/// A copy of the repo's docs under `tmp/`, safe to break.
fn docs_copy(name: &str) -> PathBuf {
    let root = repo_root();
    let copy = root.join("tmp/test-docs").join(name);
    if copy.exists() {
        fs::remove_dir_all(&copy).unwrap();
    }
    fs::create_dir_all(copy.join("docs/guidelines")).unwrap();
    for file in ["CLAUDE.md", "AGENTS.md"] {
        fs::copy(root.join(file), copy.join(file)).unwrap();
    }
    for guideline in GUIDELINES {
        let rel = Path::new("docs/guidelines").join(guideline.file);
        fs::copy(root.join(&rel), copy.join(&rel)).unwrap();
    }
    copy
}

fn edit(path: &Path, from: &str, to: &str) {
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "{} has no {from:?}", path.display());
    fs::write(path, text.replace(from, to)).unwrap();
}

#[test]
fn rejects_missing_guideline_link() {
    check_docs(repo_root()).unwrap();

    let copy = docs_copy("missing-link");
    check_docs(&copy).unwrap();
    edit(&copy.join("CLAUDE.md"), "](docs/guidelines/axum.md)", "]()");
    let err = check_docs(&copy).unwrap_err().to_string();
    assert!(
        err.contains("CLAUDE.md does not link docs/guidelines/axum.md"),
        "{err}"
    );

    let copy = docs_copy("broken-guideline");
    let leptos = copy.join("docs/guidelines/leptos.md");
    edit(
        &leptos,
        "## Sources\n",
        "## Sources\n- https://example.com/leptos-notes\n",
    );
    edit(&leptos, "0.21.14", "0.21.9");
    fs::remove_file(copy.join("docs/guidelines/playwright.md")).unwrap();
    let err = check_docs(&copy).unwrap_err().to_string();
    assert!(
        err.contains("leptos.md: `## Sources` cites https://example.com/leptos-notes"),
        "{err}"
    );
    assert!(
        err.contains("leptos.md: missing pinned version `trunk` 0.21.14"),
        "{err}"
    );
    assert!(err.contains("docs/guidelines/playwright.md:"), "{err}");

    let copy = docs_copy("agents");
    fs::write(
        copy.join("AGENTS.md"),
        "Read CLAUDE.md.\n\nAlso run the tests.\n",
    )
    .unwrap();
    let err = check_docs(&copy).unwrap_err().to_string();
    assert!(err.contains("AGENTS.md must hold only"), "{err}");
}

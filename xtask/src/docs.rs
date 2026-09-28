//! `cargo xtask check-docs`: the Q30 guideline files, `CLAUDE.md` and `AGENTS.md`.

use std::path::Path;

use anyhow::{Context, Result, bail};

#[derive(Debug)]
pub struct Guideline {
    pub file: &'static str,
    /// `(name, version)` pairs that must appear together on one line.
    pub pins: &'static [(&'static str, &'static str)],
    /// URL prefixes of the official documentation `## Sources` may cite.
    pub official: &'static [&'static str],
}

/// The Q30 guideline files under `docs/guidelines/`.
pub const GUIDELINES: &[Guideline] = &[
    Guideline {
        file: "rust.md",
        pins: &[("rustc", "1.97.1"), ("edition", "2024")],
        official: &[
            "https://doc.rust-lang.org/",
            "https://rust-lang.github.io/",
            "https://docs.rs/",
            "https://github.com/rust-lang/",
            "https://github.com/matklad/cargo-xtask",
        ],
    },
    Guideline {
        file: "llama-cpp.md",
        pins: &[("llama-cpp-2", "0.1.157")],
        official: &[
            "https://docs.rs/llama-cpp-2",
            "https://github.com/utilityai/llama-cpp-rs",
            "https://github.com/ggml-org/llama.cpp",
            "https://crates.io/crates/llama-cpp-2",
        ],
    },
    Guideline {
        file: "huggingface.md",
        pins: &[("tokenizers", "0.23.2"), ("hf-hub", "1.0.0")],
        official: &[
            "https://huggingface.co/docs/",
            "https://docs.rs/tokenizers",
            "https://docs.rs/hf-hub",
            "https://docs.rs/minijinja",
            "https://docs.rs/minijinja-contrib",
            "https://github.com/huggingface/",
            "https://github.com/mitsuhiko/minijinja",
        ],
    },
    Guideline {
        file: "axum.md",
        pins: &[
            ("axum", "0.8"),
            ("tower-http", "0.7"),
            ("memory-serve", "2.4"),
        ],
        official: &[
            "https://docs.rs/axum",
            "https://docs.rs/tower-http",
            "https://docs.rs/tower",
            "https://docs.rs/memory-serve",
            "https://docs.rs/tokio",
            "https://github.com/tokio-rs/axum",
            "https://github.com/tower-rs/tower-http",
            "https://github.com/tweedegolf/memory-serve",
            "https://tokio.rs/",
        ],
    },
    Guideline {
        file: "leptos.md",
        pins: &[
            ("leptos", "0.8"),
            ("trunk", "0.21.14"),
            ("binaryen", "version_133"),
            ("lz-str", "0.2"),
        ],
        official: &[
            "https://book.leptos.dev/",
            "https://docs.rs/leptos",
            "https://leptos.dev/",
            "https://github.com/leptos-rs/leptos",
            "https://trunkrs.dev/",
            "https://github.com/trunk-rs/trunk",
            "https://wasm-bindgen.github.io/",
            "https://rustwasm.github.io/",
            "https://github.com/WebAssembly/binaryen",
            "https://docs.rs/lz-str",
            "https://developer.mozilla.org/",
        ],
    },
    Guideline {
        file: "playwright.md",
        pins: &[
            ("@playwright/test", "1.63.0"),
            ("lz-string", "1.5.0"),
            ("@typesafe-ai/sdk", "0.6.0"),
        ],
        official: &[
            "https://playwright.dev/",
            "https://github.com/microsoft/playwright",
            "https://docs.npmjs.com/",
            "https://github.com/pieroxy/lz-string",
            "https://nodejs.org/",
            "https://www.npmjs.com/package/@typesafe-ai/sdk",
            "https://docs.typesafe.ai/",
        ],
    },
    Guideline {
        file: "python-tooling.md",
        pins: &[
            ("uv", "0.7.3"),
            ("jevcompat", "0.1.0"),
            ("typesafe-sdk", "0.7.2"),
        ],
        official: &[
            "https://docs.astral.sh/uv/",
            "https://docs.python.org/",
            "https://pip.pypa.io/",
            "https://packaging.python.org/",
            "https://github.com/astral-sh/uv",
        ],
    },
];

/// Rules `CLAUDE.md` must state, each recognised by a phrase it contains.
const CLAUDE_RULES: &[(&str, &str)] = &[
    ("the Q21 sandbox rule", "gitignored `tmp/`"),
    ("the Q26 agent environment rule", "cargo xtask env --claude"),
    ("the Q29 git rule", "no remote"),
    ("the Q23 /impeccable rule for UI work", "/impeccable"),
    ("the end-to-end verification rule", "official Hugging Face"),
    ("the guideline update rule", "same commit"),
];

/// Fails listing every file that breaks a rule, and what it breaks.
pub fn check_docs(repo_root: &Path) -> Result<()> {
    let mut problems = Vec::new();
    for guideline in GUIDELINES {
        let path = Path::new("docs/guidelines").join(guideline.file);
        match std::fs::read_to_string(repo_root.join(&path)) {
            Ok(text) => problems.extend(
                check_guideline(guideline, &text)
                    .into_iter()
                    .map(|problem| format!("{}: {problem}", path.display())),
            ),
            Err(err) => problems.push(format!("{}: {err}", path.display())),
        }
    }

    let claude = read(repo_root, "CLAUDE.md")?;
    for guideline in GUIDELINES {
        let link = format!("](docs/guidelines/{})", guideline.file);
        if !claude.contains(&link) {
            problems.push(format!(
                "CLAUDE.md does not link docs/guidelines/{}",
                guideline.file
            ));
        }
    }
    for (rule, phrase) in CLAUDE_RULES {
        if !claude.contains(phrase) {
            problems.push(format!(
                "CLAUDE.md does not state {rule} (expected `{phrase}`)"
            ));
        }
    }

    let agents = read(repo_root, "AGENTS.md")?;
    let lines: Vec<&str> = agents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() != 1 || !lines[0].contains("CLAUDE.md") {
        problems
            .push("AGENTS.md must hold only the instruction to read and follow CLAUDE.md".into());
    }

    if !problems.is_empty() {
        bail!("documentation check failed:\n  {}", problems.join("\n  "));
    }
    Ok(())
}

fn read(repo_root: &Path, file: &str) -> Result<String> {
    std::fs::read_to_string(repo_root.join(file)).with_context(|| format!("reading {file}"))
}

fn check_guideline(guideline: &Guideline, text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, version) in guideline.pins {
        if !text
            .lines()
            .any(|line| line.contains(name) && line.contains(version))
        {
            problems.push(format!("missing pinned version `{name}` {version}"));
        }
    }
    if section(text, "Rules").is_none_or(|body| body.trim().is_empty()) {
        problems.push("missing a non-empty `## Rules` section".into());
    }
    match section(text, "Sources") {
        None => problems.push("missing a `## Sources` section".into()),
        Some(body) => {
            let urls: Vec<&str> = body.split_whitespace().filter_map(url_in).collect();
            if urls.is_empty() {
                problems.push("`## Sources` cites no URL".into());
            }
            for url in urls {
                if !guideline
                    .official
                    .iter()
                    .any(|prefix| url.starts_with(prefix))
                {
                    problems.push(format!(
                        "`## Sources` cites {url}, not official documentation"
                    ));
                }
            }
        }
    }
    problems
}

/// The URL inside a markdown word such as `<https://a/b>` or `[x](http://a/b),`.
fn url_in(word: &str) -> Option<&str> {
    let separator = word.find("://")?;
    let start = word[..separator]
        .rfind(|c: char| !c.is_ascii_alphabetic())
        .map_or(0, |at| at + 1);
    Some(word[start..].trim_end_matches([')', '>', ',', '.', ';']))
}

/// The body of the `## <heading>` section, up to the next `## ` heading.
fn section<'a>(text: &'a str, heading: &str) -> Option<&'a str> {
    let marker = format!("## {heading}\n");
    let start = text.find(&marker)? + marker.len();
    let rest = &text[start..];
    Some(rest.find("\n## ").map_or(rest, |end| &rest[..end]))
}

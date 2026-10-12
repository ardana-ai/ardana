//! `cargo xtask e2e <suite>`: end-to-end suites, each run under the home guard. The API suites (`jevcompat`, `sdk`,
//! `jevbench`) run their harness from `tmp/` against a release `ardana serve` on decider:2b; the browser suites
//! (`playground`, `standalone`, `design`) live in [`crate::browser`], and the `candle` suite, which checks the rows
//! of [`CANDLE_FIXTURES`], in [`crate::candle`].

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::sandbox::{Sandbox, cargo};
use crate::serve::{Server, build_ardana};
use crate::{browser, candle};

pub const SUITES: &[&str] = &[
    "smoke",
    "rust",
    "jevcompat",
    "sdk",
    "jevbench",
    "playground",
    "standalone",
    "design",
    "candle",
];

/// A Hub repository `<org>/<name>` at one commit.
#[derive(Debug)]
pub struct Pin {
    pub repo: &'static str,
    pub commit: &'static str,
}

/// One checkpoint the `candle` suite verifies.
#[derive(Debug)]
pub struct CandleFixture {
    /// The row's name in the suite's output, and the name its checkpoint is pulled under.
    pub name: &'static str,
    /// The safetensors repository Candle loads.
    pub checkpoint: Pin,
    /// The GGUF repository of the same model and the file in it llama.cpp loads, which the checkpoint's answers are
    /// compared with.
    pub gguf: Option<(Pin, &'static str)>,
    /// Whether the row also answers a state of more than 4,500 tokens.
    pub long_state: bool,
}

/// The checkpoints `cargo xtask e2e candle` pulls, checks and removes, in run order.
pub const CANDLE_FIXTURES: &[CandleFixture] = &[
    CandleFixture {
        name: "qwen2.5-1.5b-instruct",
        checkpoint: Pin {
            repo: "Qwen/Qwen2.5-1.5B-Instruct",
            commit: "989aa7980e4cf806f80c7fef2b1adb7bc71aa306",
        },
        gguf: Some((
            Pin {
                repo: "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
                commit: "91cad51170dc346986eccefdc2dd33a9da36ead9",
            },
            "qwen2.5-1.5b-instruct-q8_0.gguf",
        )),
        long_state: true,
    },
    CandleFixture {
        name: "smollm3-3b",
        checkpoint: Pin {
            repo: "HuggingFaceTB/SmolLM3-3B",
            commit: "a07cc9a04f16550a088caea529712d1d335b0ac1",
        },
        gguf: Some((
            Pin {
                repo: "ggml-org/SmolLM3-3B-GGUF",
                commit: "4965cb60b150737b68a0408c36aeefb65078f894",
            },
            "SmolLM3-Q8_0.gguf",
        )),
        long_state: false,
    },
    CandleFixture {
        name: "llama-3.2-1b",
        checkpoint: Pin {
            repo: "unsloth/Llama-3.2-1B-Instruct",
            commit: "5a8abab4a5d6f164389b1079fb721cfab8d7126c",
        },
        gguf: Some((
            Pin {
                repo: "bartowski/Llama-3.2-1B-Instruct-GGUF",
                commit: "067b946cf014b7c697f3654f621d577a3e3afd1c",
            },
            "Llama-3.2-1B-Instruct-Q8_0.gguf",
        )),
        long_state: false,
    },
    CandleFixture {
        name: "minicpm5-1b",
        checkpoint: Pin {
            repo: "openbmb/MiniCPM5-1B",
            commit: "87179e5c1f455ef22e6223592d2d61351b525bfc",
        },
        gguf: Some((
            Pin {
                repo: "openbmb/MiniCPM5-1B-GGUF",
                commit: "3d55fac80935ae6456986ad2384b5cbcc4d6c948",
            },
            "MiniCPM5-1B-Q8_0.gguf",
        )),
        long_state: false,
    },
    CandleFixture {
        name: "phi-4-mini",
        checkpoint: Pin {
            repo: "microsoft/Phi-4-mini-instruct",
            commit: "cfbefacb99257ffa30c83adab238a50856ac3083",
        },
        gguf: Some((
            Pin {
                repo: "unsloth/Phi-4-mini-instruct-GGUF",
                commit: "78eb92a46fc37e6b524df991ed9aca9bc6aa7b80",
            },
            "Phi-4-mini-instruct.Q8_0.gguf",
        )),
        long_state: true,
    },
    CandleFixture {
        name: "granite-4.2-3b",
        checkpoint: Pin {
            repo: "ibm-granite/granite-4.2-3b",
            commit: "e459acceac81e5fe67c07d9cfc72329a332e7eb1",
        },
        gguf: Some((
            Pin {
                repo: "ibm-granite/granite-4.2-3b-GGUF",
                commit: "c40945d71cd90f249a56985e8155551a9188dc30",
            },
            "granite-4.2-3b-Q8_0.gguf",
        )),
        long_state: false,
    },
    CandleFixture {
        name: "lfm2.5-1.2b",
        checkpoint: Pin {
            repo: "LiquidAI/LFM2.5-1.2B-Instruct",
            commit: "0f604ada3f766f9f257460c4c9f0b5d6f69d431b",
        },
        gguf: Some((
            Pin {
                repo: "LiquidAI/LFM2.5-1.2B-Instruct-GGUF",
                commit: "8ed288026e23958ad9dfa92d53ed773a8eee7125",
            },
            "LFM2.5-1.2B-Instruct-Q8_0.gguf",
        )),
        long_state: true,
    },
    CandleFixture {
        name: "gemma-3-1b",
        checkpoint: Pin {
            repo: "unsloth/gemma-3-1b-it",
            commit: "5b11413a10db4e486ef16a20101fd028f8f2499c",
        },
        gguf: Some((
            Pin {
                repo: "ggml-org/gemma-3-1b-it-GGUF",
                commit: "f9c28bcd85737ffc5aef028638d3341d49869c27",
            },
            "gemma-3-1b-it-Q8_0.gguf",
        )),
        long_state: true,
    },
    CandleFixture {
        name: "gemma-3-4b",
        checkpoint: Pin {
            repo: "unsloth/gemma-3-4b-it",
            commit: "bf46152c47f5dd20b896357cb51abc4c03b8ee8c",
        },
        gguf: Some((
            Pin {
                repo: "ggml-org/gemma-3-4b-it-GGUF",
                commit: "d0976223747697cb51e056d85c532013931fe52e",
            },
            "gemma-3-4b-it-Q8_0.gguf",
        )),
        long_state: true,
    },
    CandleFixture {
        name: "olmo-3-7b",
        checkpoint: Pin {
            repo: "allenai/Olmo-3-7B-Instruct",
            commit: "6e5971d9eba42665f5bd5a0fcf047f299ce1dccc",
        },
        gguf: Some((
            Pin {
                repo: "unsloth/Olmo-3-7B-Instruct-GGUF",
                commit: "a2a9ae53f5a95087616b3f45ca96e9d2631344ee",
            },
            "Olmo-3-7B-Instruct-Q8_0.gguf",
        )),
        long_state: true,
    },
];

/// The request fixture the SDK suite sends.
const SDK_REQUEST: &str = "tests/fixtures/requests/ticket.json";
/// JevBench's public items, in the order its results report them.
const JEVBENCH_TASKS: &[&str] = &[
    "datasets/public/easy.jsonl",
    "datasets/public/original.jsonl",
    "datasets/public/hard.jsonl",
];

pub fn run(sandbox: &Sandbox, suite: &str) -> Result<()> {
    match suite {
        "smoke" => smoke(sandbox),
        "rust" => rust(sandbox),
        "jevcompat" => jevcompat(sandbox),
        "sdk" => sdk(sandbox),
        "jevbench" => jevbench(sandbox),
        "playground" => browser::playground(sandbox),
        "standalone" => browser::standalone(sandbox),
        "design" => browser::design(sandbox),
        "candle" => candle::candle(sandbox, CANDLE_FIXTURES),
        other => bail!("unknown e2e suite `{other}`; suites: {}", SUITES.join(", ")),
    }
}

/// Builds and runs `ardana --version`.
fn smoke(sandbox: &Sandbox) -> Result<()> {
    let output = sandbox
        .command(cargo())
        .current_dir(sandbox.repo_root())
        .args(["run", "--quiet", "--package", "ardana", "--", "--version"])
        .output()
        .context("running ardana --version")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || !stdout.starts_with("ardana ") {
        bail!(
            "ardana --version failed ({}): {}{}",
            output.status,
            stdout.trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    println!("e2e smoke: {}", stdout.trim());
    Ok(())
}

/// Every workspace test, including the `#[ignore]`d end-to-end tests on real models.
fn rust(sandbox: &Sandbox) -> Result<()> {
    let status = sandbox
        .command(cargo())
        .current_dir(sandbox.repo_root())
        .args(["test", "--workspace", "--", "--include-ignored"])
        .status()
        .context("running cargo test")?;
    if !status.success() {
        bail!("cargo test --workspace -- --include-ignored failed ({status})");
    }
    Ok(())
}

/// An emptied `tmp/evals/<name>`.
fn evals_dir(sandbox: &Sandbox, name: &str) -> Result<std::path::PathBuf> {
    let dir = sandbox.tmp().join("evals").join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("emptying {}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// R5.1: `jevcompat test` against an open server and against one that requires a key (read from `ARDANA_API_KEY`),
/// so both conditional auth MUSTs are tested; each run must be conformant. Reports go to `tmp/evals/jevcompat/`.
fn jevcompat(sandbox: &Sandbox) -> Result<()> {
    let ardana = build_ardana(sandbox)?;
    let out = evals_dir(sandbox, "jevcompat")?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let key = format!("ardana-e2e-{nanos:x}");
    for (label, key) in [("open", None), ("key", Some(key.as_str()))] {
        let server = Server::start(sandbox, &ardana, &format!("jevcompat-{label}"), key)?;
        let report = out.join(format!("{label}.json"));
        let mut test = sandbox.command("jevcompat");
        test.args(["test", &server.url, "--json"]).arg(&report);
        if let Some(key) = key {
            test.args(["--key-env", "JEVCOMPAT_KEY"])
                .env("JEVCOMPAT_KEY", key);
        }
        let status = test.status().context("running jevcompat test")?;
        server.stop()?;
        let summary = read_json(&report)?["summary"].clone();
        let count = |level: &str, what: &str| summary[level][what].as_u64().unwrap_or_default();
        println!(
            "e2e jevcompat ({label}): {} · MUST {}/{} passed, {} failed, {} skipped · SHOULD {}/{} passed",
            summary["verdict"].as_str().unwrap_or("?"),
            count("MUST", "passed"),
            count("MUST", "tested"),
            count("MUST", "failed"),
            count("MUST", "skipped"),
            count("SHOULD", "passed"),
            count("SHOULD", "tested"),
        );
        if !status.success()
            || summary["conformant"] != Value::Bool(true)
            || count("MUST", "failed") != 0
        {
            bail!(
                "jevcompat ({label}) is not conformant ({status}); see {}",
                report.display()
            );
        }
    }
    Ok(())
}

/// R5.2: `typesafe-sdk` from `tmp/py/sdk`, pointed at the server by `TYPESAFE_BASE_URL`, parses the ticket answers.
fn sdk(sandbox: &Sandbox) -> Result<()> {
    let ardana = build_ardana(sandbox)?;
    let server = Server::start(sandbox, &ardana, "sdk", None)?;
    let root = sandbox.repo_root();
    let status = sandbox
        .command(sandbox.tmp().join("py/sdk/bin/python"))
        .current_dir(root)
        .arg(root.join("xtask/scripts/sdk_ticket.py"))
        .arg(root.join(SDK_REQUEST))
        .env("TYPESAFE_BASE_URL", &server.url)
        .env("TYPESAFE_API_KEY", "ardana-e2e-unused")
        .status()
        .context("running the SDK check")?;
    server.stop()?;
    if !status.success() {
        bail!("typesafe-sdk did not parse the answers to {SDK_REQUEST} ({status})");
    }
    Ok(())
}

/// R5.3: JevBench's 231 public items through its `typesafe` adapter, run from the `tmp/src/jevbench` clone with the
/// `tmp/py/jevbench` venv; results, ledger, raw responses and summary land in `tmp/evals/jevbench/`. Every request
/// must succeed; accuracy is reported, not gated.
fn jevbench(sandbox: &Sandbox) -> Result<()> {
    let ardana = build_ardana(sandbox)?;
    let out = evals_dir(sandbox, "jevbench")?;
    let clone = sandbox.tmp().join("src/jevbench");
    let python = sandbox.tmp().join("py/jevbench/bin/python");
    let tasks = JEVBENCH_TASKS
        .iter()
        .map(|file| clone.join(file).display().to_string())
        .collect::<Vec<_>>()
        .join(",");
    let results = out.join("results.jsonl");
    let ledger = out.join("ledger.jsonl");
    let server = Server::start(sandbox, &ardana, "jevbench", None)?;
    let status = sandbox
        .command(&python)
        .current_dir(&clone)
        .args([
            "-m",
            "jevbench.cli",
            "run",
            "--adapter",
            "typesafe",
            "--key-env",
            "",
            "--endpoint",
        ])
        .arg(&server.url)
        .arg("--tasks")
        .arg(&tasks)
        .arg("--results")
        .arg(&results)
        .arg("--ledger")
        .arg(&ledger)
        .arg("--raw-dir")
        .arg(out.join("raw"))
        .status()
        .context("running jevbench run")?;
    server.stop()?;

    let summary = sandbox
        .command(&python)
        .current_dir(&clone)
        .args(["-m", "jevbench.cli", "summarize", "--tasks"])
        .arg(&tasks)
        .arg("--results")
        .arg(&results)
        .arg("--ledger")
        .arg(&ledger)
        .output()
        .context("running jevbench summarize")?;
    if !summary.status.success() {
        bail!(
            "jevbench summarize failed ({}): {}",
            summary.status,
            String::from_utf8_lossy(&summary.stderr)
        );
    }
    std::fs::write(out.join("summary.json"), &summary.stdout)?;
    let summary: Value =
        serde_json::from_slice(&summary.stdout).context("parsing the JevBench summary")?;

    let records = std::fs::read_to_string(&results)
        .with_context(|| format!("reading {}", results.display()))?;
    let records: Vec<Value> = records
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .context("parsing the JevBench results")?;
    let failed: Vec<&Value> = records.iter().filter(|r| r["status"] != "ok").collect();
    let planned = summary["n_planned"].as_u64().unwrap_or_default();
    println!(
        "e2e jevbench: {} of {planned} requests, {} failed; accuracy {} ({} of {} scorable), macro accuracy {}; {}",
        records.len(),
        failed.len(),
        summary["accuracy"],
        summary["n_correct"],
        summary["n_scorable"],
        summary["macro_accuracy"],
        out.display()
    );
    for record in failed.iter().take(5) {
        println!("  failed {}: {}", record["task_id"], record["error"]);
    }
    if !status.success() || records.len() as u64 != planned || !failed.is_empty() {
        bail!("jevbench run had failed or missing requests ({status})");
    }
    Ok(())
}

pub(crate) fn read_json(path: &Path) -> Result<Value> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

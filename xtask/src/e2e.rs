//! `cargo xtask e2e <suite>`: end-to-end suites, each run under the home guard. `rust` runs the plain workspace tests,
//! then the rows of [`RUST_FIXTURES`]: each downloads its weights, runs its `#[ignore]`d tests and removes the weights.
//! The API suites (`jevcompat`, `sdk`, `jevbench`) run their harness from `tmp/` against a release `ardana serve` on
//! decider:2b; the browser suites (`playground`, `standalone`, `design`) live in [`crate::browser`], the `candle`
//! suite, which checks the rows of [`CANDLE_FIXTURES`], in [`crate::candle`], and the `finetune` suite, which runs the
//! rows of [`FINETUNE_FIXTURES`], in [`crate::finetune`]. Every suite but `candle` and `finetune`, which pull through
//! `ardana`, gets the weights it reads from [`hub::with_weights`], and leaves `tmp/hf` as it found it.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

use crate::sandbox::{Sandbox, cargo, run as run_command};
use crate::serve::{Server, build_ardana};
use crate::{browser, candle, finetune, hub};

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
    "finetune",
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
    /// compared with; `None` for the THX-01 decision model, whose answers are compared with the `thx01` package's
    /// goldens in `crates/ardana-core/tests/data/thx01/`.
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
    CandleFixture {
        name: "thx-01",
        checkpoint: Pin {
            repo: "doofz/THX-01",
            commit: "bef2e2478be7acd79429324a658558a57552a2e8",
        },
        gguf: None,
        long_state: false,
    },
];

/// One fine-tune the `finetune` suite runs.
#[derive(Debug)]
pub struct FinetuneFixture {
    /// The row's name in the suite's output.
    pub name: &'static str,
    /// The base `ardana finetune` is given.
    pub base: &'static str,
    /// The arguments after the base: the data and questions files, which the suite writes into its directory and
    /// names relative to it, `--name` and any other flag.
    pub args: &'static [&'static str],
    pub expect: Expect,
    /// What the model's test accuracy must be against the base's.
    pub gain: Gain,
}

/// What a fine-tune row expects of `ardana finetune`.
#[derive(Debug)]
pub enum Expect {
    /// It exits 0, its model is registered and `ardana run` answers with it.
    Answers,
    /// It exits 1 before downloading any weight, its message holding this text.
    Refused(&'static str),
}

/// The overall test accuracy a fine-tune row expects of its model against the base's.
#[derive(Debug, Clone, Copy)]
pub enum Gain {
    /// Above the base's.
    Above,
    /// At least the base's minus this.
    Within(f64),
    /// Not compared: the row makes no model, or what it checks of its model is that it answers.
    Unchecked,
}

/// The fine-tunes `cargo xtask e2e finetune` runs, in run order, on one suite home.
pub const FINETUNE_FIXTURES: &[FinetuneFixture] = &[
    FinetuneFixture {
        name: "qwen3:0.6b",
        base: "qwen3:0.6b",
        args: &[
            finetune::COMMANDS,
            "--questions",
            finetune::COMMANDS_QUESTIONS,
            "--name",
            "cmd-risk",
        ],
        expect: Expect::Answers,
        gain: Gain::Above,
    },
    FinetuneFixture {
        name: "thx-01",
        base: "hf.co/doofz/THX-01",
        args: &[
            finetune::COMMANDS,
            "--questions",
            finetune::COMMANDS_QUESTIONS,
            "--name",
            "thx-risk",
        ],
        expect: Expect::Refused("is a decision model"),
        gain: Gain::Unchecked,
    },
    // decider's plain layout with isolated score levels, on questions of its own training mixture: no loss.
    FinetuneFixture {
        name: "decider:0.8b",
        base: "decider:0.8b",
        args: &[
            finetune::CUSTOM,
            "--name",
            "custom-decider",
            "--epochs",
            "1",
        ],
        expect: Expect::Answers,
        gain: Gain::Within(0.05),
    },
    // A multimodal checkpoint (`Qwen3_5ForConditionalGeneration`) in the chat layout.
    FinetuneFixture {
        name: "qwen3.5:0.8b",
        base: "qwen3.5:0.8b",
        args: &[
            finetune::COMMANDS,
            "--questions",
            finetune::COMMANDS_QUESTIONS,
            "--name",
            "cmd-risk-qwen3.5",
        ],
        expect: Expect::Answers,
        gain: Gain::Above,
    },
    // An `hf.co/` GGUF reference on the CSV data: its checkpoint the base_model its card names (Qwen/Qwen3-0.6B), then
    // the same GGUF as a local file with `--checkpoint <that snapshot directory>` and `--epochs 1`. On the CPU, in
    // float32, where the other rows train on Apple's GPU on a Mac.
    FinetuneFixture {
        name: "hf.co-gguf",
        base: "hf.co/Qwen/Qwen3-0.6B-GGUF:Q8_0",
        args: &[
            finetune::COMMANDS_CSV,
            "--questions",
            finetune::COMMANDS_QUESTIONS,
            "--name",
            "cmd-risk-gguf",
            "--device",
            "cpu",
        ],
        expect: Expect::Answers,
        gain: Gain::Unchecked,
    },
    // A checkpoint whose `config.json` has `auto_map` (`Spark2_5ForCausalLM`): refused without `--trust-remote-code`,
    // then run with it, one epoch, as far as the trainer importing the checkpoint's own code (R3.5 as amended: that
    // code targets transformers 4.57 and does not load under the pinned 5.18).
    FinetuneFixture {
        name: "spark-x2.5:1.7b",
        base: "spark-x2.5:1.7b",
        args: &[
            finetune::COMMANDS,
            "--questions",
            finetune::COMMANDS_QUESTIONS,
            "--name",
            "cmd-risk-spark",
            "--epochs",
            "1",
        ],
        expect: Expect::Refused("--trust-remote-code"),
        gain: Gain::Unchecked,
    },
    // The merged checkpoint itself (`Qwen3ForCausalLM`), which Candle runs, calibrates through and reports on.
    FinetuneFixture {
        name: "safetensors",
        base: "hf.co/Qwen/Qwen3-0.6B",
        args: &[
            finetune::COMMANDS,
            "--questions",
            finetune::COMMANDS_QUESTIONS,
            "--name",
            "cmd-risk-st",
            "--format",
            "safetensors",
        ],
        expect: Expect::Answers,
        gain: Gain::Above,
    },
    // A text-only checkpoint (`Gemma3ForCausalLM`) whose tokenizer_config.json names a multimodal processor
    // (`Gemma3Processor`), which the trainer reads through a copy without it.
    FinetuneFixture {
        name: "gemma-3:1b",
        base: "gemma-3:1b",
        args: &[
            finetune::COMMANDS,
            "--questions",
            finetune::COMMANDS_QUESTIONS,
            "--name",
            "cmd-risk-gemma3",
            "--epochs",
            "1",
        ],
        expect: Expect::Answers,
        gain: Gain::Above,
    },
    // A multimodal Gemma (`Gemma4ForConditionalGeneration`): its text tower loaded through a key mapping from the
    // checkpoint's `model.language_model.` tensors, which transformers does not map itself, and merged under the text
    // model's own names.
    FinetuneFixture {
        name: "gemma-4:e2b",
        base: "gemma-4:e2b",
        args: &[
            finetune::COMMANDS,
            "--questions",
            finetune::COMMANDS_QUESTIONS,
            "--name",
            "cmd-risk-gemma4",
            "--epochs",
            "1",
        ],
        expect: Expect::Answers,
        gain: Gain::Above,
    },
    // A checkpoint whose `config.json` has `auto_map` (Phi-4-mini's `Phi3ForCausalLM`) for a model transformers
    // implements itself: trained by transformers' own classes, without `--trust-remote-code`, its code never run.
    FinetuneFixture {
        name: "phi-4:mini",
        base: "phi-4:mini",
        args: &[
            finetune::COMMANDS,
            "--questions",
            finetune::COMMANDS_QUESTIONS,
            "--name",
            "cmd-risk-phi4",
            "--epochs",
            "1",
        ],
        expect: Expect::Answers,
        gain: Gain::Above,
    },
    // The device and memory limits, on a machine without CUDA: qwen3.8:27b on the CPU, about 260 GB in float32, exits 1
    // naming that estimate and the machine's memory, with `--device cuda` exits 1 naming the devices torch finds, and on
    // a Mac of 64 GiB or less with `--device mps` exits 1 naming the working set Metal recommends; no run downloads a
    // weight.
    FinetuneFixture {
        name: "limits",
        base: "qwen3.8:27b",
        args: &[
            finetune::COMMANDS,
            "--questions",
            finetune::COMMANDS_QUESTIONS,
            "--name",
            "cmd-risk-27b",
            "--device",
            "cpu",
        ],
        expect: Expect::Refused("of memory to train on cpu in float32, more than the"),
        gain: Gain::Unchecked,
    },
];

/// One row of the `rust` suite: the `#[ignore]`d workspace tests that need the same weights.
#[derive(Debug)]
pub struct RustFixture {
    /// The row's name in the suite's output: the ardana name of the model its tests run, or what they exercise when
    /// they need several models together.
    pub name: &'static str,
    /// The `[[hf]]` repositories of `xtask/fetch.toml` whose weights the row downloads and removes; none for a row
    /// that reads no model.
    pub weights: &'static [&'static str],
    /// The tests the row runs, as libtest names them.
    pub tests: &'static [&'static str],
}

/// The rows `cargo xtask e2e rust` runs after the plain tests, in run order. Every `#[ignore]`d test of the workspace
/// is in exactly one ([`drift`]).
pub const RUST_FIXTURES: &[RustFixture] = &[
    RustFixture {
        name: "decider:2b",
        weights: &["Mapika/decider-2b-GGUF"],
        tests: &[
            "decider_2b_ticket",
            "pull_decider",
            "pull_local",
            "run_pulls_a_library_model_first",
            "serve_pulls_the_default_model_on_first_use",
            "run_inline_questions",
            "run_says_a_newer_release",
        ],
    },
    RustFixture {
        name: "qwen3.5:0.8b",
        weights: &["ggml-org/Qwen3.5-0.8B-GGUF"],
        tests: &[
            "qwen35_0_8b",
            "stock_profile",
            "serve_picks_up_models_pulled_while_it_runs",
            "pull_ollama",
        ],
    },
    RustFixture {
        name: "smollm3:3b",
        weights: &["ggml-org/SmolLM3-3B-GGUF"],
        tests: &["smollm3_3b"],
    },
    RustFixture {
        name: "gemma-4:e2b",
        weights: &["ggml-org/gemma-4-E2B-it-GGUF"],
        tests: &["gemma4_e2b"],
    },
    // decider:2b, qwen3.5:0.8b, smollm3:3b and gemma-4:e2b in one registry.
    RustFixture {
        name: "library",
        weights: &[
            "Mapika/decider-2b-GGUF",
            "ggml-org/Qwen3.5-0.8B-GGUF",
            "ggml-org/SmolLM3-3B-GGUF",
            "ggml-org/gemma-4-E2B-it-GGUF",
        ],
        tests: &[
            "pull_library_names",
            "pull_reads_the_pinned_commit",
            "pull_stock",
            "list_show_rm",
            "library_names_are_spellings",
            "run_by_name",
        ],
    },
    // The Candle runtime's checkpoint and the GGUF of the same model.
    RustFixture {
        name: "qwen3:1.7b",
        weights: &["Qwen/Qwen3-1.7B", "Qwen/Qwen3-1.7B-GGUF"],
        tests: &[
            "tests::qwen3_decodes_each_prompt_alone",
            "pull_safetensors",
            "pull_with_a_tokenizer_asks_for_no_other",
            "candle_answers_like_the_gguf",
        ],
    },
    // decider:0.8b's browser variant.
    RustFixture {
        name: "decider:0.8b",
        weights: &["ardana-ai/decider-0.8b-ONNX"],
        tests: &["browser_files"],
    },
    // The network alone: the latest GitHub Release.
    RustFixture {
        name: "update",
        weights: &[],
        tests: &["latest_release_is_installed"],
    },
];

/// The weights the API suites serve: decider:2b's GGUF ([`crate::serve::MODEL`]).
const SERVED_WEIGHTS: &[&str] = &["Mapika/decider-2b-GGUF"];
/// The weights the browser suites read: the models their servers pull (decider:2b, decider:0.8b, qwen3.5:0.8b) and
/// every browser variant of the library.
const BROWSER_WEIGHTS: &[&str] = &[
    "Mapika/decider-2b-GGUF",
    "ardana-ai/decider-0.8b-GGUF",
    "ggml-org/Qwen3.5-0.8B-GGUF",
    "ardana-ai/decider-0.8b-ONNX",
    "ardana-ai/decider-2b-ONNX",
    "ardana-ai/qwen3.5-0.8b-ONNX",
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
    let what = format!("e2e {suite}");
    let served = |suite: fn(&Sandbox) -> Result<()>| {
        hub::with_weights(sandbox, &what, SERVED_WEIGHTS, || suite(sandbox))
    };
    let in_browser = |suite: fn(&Sandbox) -> Result<()>| {
        hub::with_weights(sandbox, &what, BROWSER_WEIGHTS, || suite(sandbox))
    };
    match suite {
        "smoke" => smoke(sandbox),
        "rust" => rust(sandbox, RUST_FIXTURES),
        "jevcompat" => served(jevcompat),
        "sdk" => served(sdk),
        "jevbench" => served(jevbench),
        "playground" => in_browser(browser::playground),
        "standalone" => in_browser(browser::standalone),
        "design" => in_browser(browser::design),
        "candle" => candle::candle(sandbox, CANDLE_FIXTURES),
        "finetune" => finetune::finetune(sandbox, FINETUNE_FIXTURES),
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

/// `cargo test` over the workspace's test targets, with `args` for the test binaries.
fn cargo_test(sandbox: &Sandbox, args: &[&str]) -> std::process::Command {
    let mut test = sandbox.command(cargo());
    test.current_dir(sandbox.repo_root())
        .args(["test", "--workspace", "--tests", "--no-fail-fast", "--"])
        .args(args);
    test
}

/// The plain workspace tests once, then each row of `fixtures` in turn: its weights downloaded into `tmp/hf`, its
/// `#[ignore]`d tests run, and every path it added to the cache removed. A failed row is counted and does not end the
/// suite. Before anything is downloaded, the rows must hold every `#[ignore]`d test of the workspace ([`drift`]) and
/// name only weights `xtask/fetch.toml` pins.
fn rust(sandbox: &Sandbox, fixtures: &[RustFixture]) -> Result<()> {
    let listed = cargo_test(sandbox, &["--list", "--ignored"])
        .stderr(Stdio::inherit())
        .output()
        .context("listing the ignored tests")?;
    ensure!(
        listed.status.success(),
        "cargo test -- --list --ignored failed ({})",
        listed.status
    );
    let listed = String::from_utf8(listed.stdout)?;
    let ignored: Vec<&str> = listed
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .collect();
    let problems = drift(&ignored, fixtures);
    ensure!(
        problems.is_empty(),
        "the rows of the rust suite and the workspace's #[ignore]d tests differ:\n  {}",
        problems.join("\n  ")
    );
    for fixture in fixtures {
        hub::pins(sandbox, fixture.weights).with_context(|| format!("row {}", fixture.name))?;
    }

    run_command(
        sandbox
            .command(cargo())
            .current_dir(sandbox.repo_root())
            .args(["test", "--workspace"]),
    )?;
    let mut failed = Vec::new();
    for fixture in fixtures {
        let what = format!("e2e rust {}", fixture.name);
        let started = Instant::now();
        let mut tests = cargo_test(sandbox, &["--ignored", "--exact"]);
        tests.args(fixture.tests);
        match hub::with_weights(sandbox, &what, fixture.weights, || run_command(&mut tests)) {
            Ok(()) => println!(
                "{what}: {} tests passed in {:.1} s",
                fixture.tests.len(),
                started.elapsed().as_secs_f64()
            ),
            Err(err) => {
                println!("{what}: failed: {err:#}");
                failed.push(fixture.name);
            }
        }
    }
    ensure!(
        failed.is_empty(),
        "{} of {} rust rows failed: {}",
        failed.len(),
        fixtures.len(),
        failed.join(", ")
    );
    Ok(())
}

/// What keeps `ignored`, the workspace's `#[ignore]`d tests as libtest lists them, from being the tests of `fixtures`,
/// each in exactly one row: a test no row lists, a row entry no test matches, a name two rows or two tests share, and
/// a row without a test (libtest would run every `#[ignore]`d test for it).
fn drift(ignored: &[&str], fixtures: &[RustFixture]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut rows: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for fixture in fixtures {
        for test in fixture.tests {
            rows.entry(test).or_default().push(fixture.name);
        }
    }
    for fixture in fixtures.iter().filter(|fixture| fixture.tests.is_empty()) {
        problems.push(format!("row {} lists no test", fixture.name));
    }
    for (test, rows) in &rows {
        let tests = ignored.iter().filter(|name| name == &test).count();
        match (tests, rows.as_slice()) {
            (0, _) => problems.push(format!(
                "row {} lists `{test}`, which no #[ignore]d test is named",
                rows.join(", ")
            )),
            (1, [_]) => {}
            (1, _) => problems.push(format!("`{test}` is in the rows {}", rows.join(", "))),
            (n, _) => problems.push(format!(
                "{n} #[ignore]d tests are named `{test}`; a row names one test"
            )),
        }
    }
    for test in ignored {
        if !rows.contains_key(test) {
            problems.push(format!("no row lists the #[ignore]d test `{test}`"));
        }
    }
    problems
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

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS: &[RustFixture] = &[
        RustFixture {
            name: "a",
            weights: &[],
            tests: &["one", "tests::two"],
        },
        RustFixture {
            name: "b",
            weights: &[],
            tests: &["three"],
        },
    ];

    /// Every `#[ignore]`d test belongs to exactly one row: a test without a row, a row entry without a test, a test
    /// in two rows and a name two tests share are each named.
    #[test]
    fn drift_names_what_differs() {
        assert_eq!(
            drift(&["three", "one", "tests::two"], ROWS),
            [] as [&str; 0]
        );
        assert_eq!(
            drift(&["one", "tests::two", "three", "four"], ROWS),
            ["no row lists the #[ignore]d test `four`"]
        );
        assert_eq!(
            drift(&["one", "three"], ROWS),
            ["row a lists `tests::two`, which no #[ignore]d test is named"]
        );
        assert_eq!(
            drift(&["one", "one", "tests::two", "three"], ROWS),
            ["2 #[ignore]d tests are named `one`; a row names one test"]
        );
        let twice = [
            RustFixture {
                name: "c",
                weights: &[],
                tests: &["one"],
            },
            RustFixture {
                name: "d",
                weights: &[],
                tests: &["one"],
            },
        ];
        assert_eq!(drift(&["one"], &twice), ["`one` is in the rows c, d"]);
        let empty = [RustFixture {
            name: "e",
            weights: &[],
            tests: &[],
        }];
        assert_eq!(drift(&[], &empty), ["row e lists no test"]);
    }

    /// The weights every suite names are `[[hf]]` entries of `xtask/fetch.toml`, each with a weight file.
    #[test]
    fn suites_name_pinned_weights() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let manifest = crate::fetch::Manifest::load(root).unwrap();
        let named = RUST_FIXTURES
            .iter()
            .flat_map(|fixture| fixture.weights)
            .chain(SERVED_WEIGHTS)
            .chain(BROWSER_WEIGHTS);
        for repo in named {
            let pin = manifest.hf.iter().find(|hf| hf.repo == *repo);
            let pin = pin.unwrap_or_else(|| panic!("fetch.toml has no [[hf]] entry for {repo}"));
            assert_ne!(
                pin.files,
                pin.without_weights().files,
                "{repo} lists no weights"
            );
        }
    }
}

//! R4.4 and R4.6: the registry commands `list`, `show` and `rm`, and `ardana run` on an Ollama blob llama.cpp cannot
//! load; the commands' guidance when input is missing, and `ardana run`'s inline questions on a real model.

mod common;

use std::path::Path;

use anyhow::{Context, Result, ensure};
use common::Ardana;

/// A GGUF v3 header: magic, version, tensor count, then `kv` string pairs.
fn gguf(tensors: u64, kv: &[(&str, &str)]) -> Vec<u8> {
    let string = |out: &mut Vec<u8>, s: &str| {
        out.extend((s.len() as u64).to_le_bytes());
        out.extend(s.as_bytes());
    };
    let mut out = b"GGUF".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(tensors.to_le_bytes());
    out.extend((kv.len() as u64).to_le_bytes());
    for (key, value) in kv {
        string(&mut out, key);
        out.extend(8u32.to_le_bytes()); // GGUF_TYPE_STRING
        string(&mut out, value);
    }
    out
}

/// Writes `ollama:library/<name>:latest` into the fake store `store` with `blob` as its model layer.
fn fake_ollama_model(store: &Path, name: &str, hex: &str, blob: &[u8]) -> Result<()> {
    let manifest_dir = store
        .join("manifests/registry.ollama.ai/library")
        .join(name);
    std::fs::create_dir_all(&manifest_dir)?;
    std::fs::create_dir_all(store.join("blobs"))?;
    std::fs::write(store.join("blobs").join(format!("sha256-{hex}")), blob)?;
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
        "layers": [{
            "mediaType": "application/vnd.ollama.image.model",
            "digest": format!("sha256:{hex}"),
            "size": blob.len(),
        }],
    });
    std::fs::write(manifest_dir.join("latest"), manifest.to_string())?;
    Ok(())
}

/// A blob with the GGUF magic that llama.cpp refuses: loading it must fail cleanly (exit 1, no abort) with a message
/// naming the `ollama:` ref and suggesting an `hf.co/` source.
#[test]
fn unloadable_ollama_blob() -> Result<()> {
    let ardana = Ardana::new("cli-unloadable-ollama-blob")?;
    let store = ardana
        .home
        .parent()
        .context("home has a parent")?
        .join("ollama");
    let blobs: [(&str, Vec<u8>); 2] = [
        // Like Ollama's own gemma3 and qwen3.5 conversions: an architecture without the keys llama.cpp needs.
        ("unconverted", gguf(0, &[("general.architecture", "llama")])),
        // A header announcing a tensor the file does not hold.
        ("truncated", gguf(1, &[])),
    ];
    for (i, (name, blob)) in blobs.iter().enumerate() {
        let hex = format!("{i:064x}");
        fake_ollama_model(&store, name, &hex, blob)?;
        let reference = format!("ollama:{name}");
        let mut pull = ardana.command([
            "pull",
            reference.as_str(),
            "--tokenizer",
            "hf.co/meta-llama/Llama-3.2-3B-Instruct",
        ]);
        pull.env("OLLAMA_MODELS", &store);
        assert!(pull.status()?.success(), "{pull:?} failed");

        let mut run = ardana.command(["run", name, "--request"]);
        run.arg(common::request("sentiment.json"));
        let output = run.output()?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("{run:?} ({}):\n{stderr}", output.status);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{name}: exits with an error, not a crash"
        );
        assert!(output.stdout.is_empty(), "{name}: no response");
        assert!(
            stderr.contains(&reference),
            "{name}: names the ref: {stderr}"
        );
        assert!(
            stderr.contains("ardana pull hf.co/"),
            "{name}: suggests an hf.co/ source: {stderr}"
        );
    }
    Ok(())
}

#[test]
#[ignore = "e2e: decider-2b and Qwen3.5-0.8B GGUFs and tokenizers in tmp/hf (cargo xtask fetch)"]
fn list_show_rm() -> Result<()> {
    let ardana = Ardana::new("cli-list-show-rm")?;
    let empty = ardana.ok(["list"])?;
    assert_eq!(empty.trim(), "NAME  SIZE  PULLED  SOURCE");

    ardana.ok(["pull", "hf.co/Mapika/decider-2b-GGUF:Q4_K_M"])?;
    ardana.ok([
        "pull",
        "hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Q4_0",
        "--tokenizer",
        "hf.co/Qwen/Qwen3.5-0.8B",
        "--name",
        "qwen3.5-0.8b",
    ])?;

    let list = ardana.ok(["list"])?;
    let rows: Vec<Vec<&str>> = list
        .lines()
        .map(|l| {
            l.split("  ")
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .collect()
        })
        .collect();
    assert_eq!(
        rows,
        [
            vec!["NAME", "SIZE", "PULLED", "SOURCE"],
            vec![
                "decider-2b",
                "1.3 GB",
                "today",
                "hf.co/Mapika/decider-2b-GGUF:Q4_K_M"
            ],
            vec![
                "qwen3.5-0.8b",
                "563.0 MB",
                "today",
                "hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Q4_0"
            ],
        ],
        "{list}"
    );

    let show: serde_json::Value =
        serde_json::from_str(&ardana.ok(["show", "decider-2b", "--json"])?)?;
    let shown = ardana.ok(["show", "decider-2b"])?;
    for line in [
        "    profile         decider-2b-v11",
        "    layout          plain (decider format)",
        "    choice          temperature 1.164",
    ] {
        assert!(shown.contains(line), "{line:?} in:\n{shown}");
    }
    let weights = common::hf_file("Mapika/decider-2b-GGUF", "decider-2b-v11-Q4_K_M.gguf")?;
    assert_eq!(show["name"], "decider-2b");
    assert_eq!(show["source"], "hf.co/Mapika/decider-2b-GGUF:Q4_K_M");
    assert_eq!(show["weights"], weights.to_string_lossy().as_ref());
    assert_eq!(show["runtime"], "llama.cpp");
    assert_eq!(show["profile"]["name"], "decider-2b-v11");
    assert_eq!(show["profile"]["layout"]["kind"], "plain");
    assert_eq!(show["profile"]["temperature_by_type"]["choice"], 1.164);
    assert!(show["pulled_at"].is_string());

    let tokenizer = common::hf_file("Mapika/decider-2b-GGUF", "tokenizer.json")?;
    let config = common::hf_file("Mapika/decider-2b-GGUF", "decider_config.json")?;
    ardana.ok(["rm", "decider-2b"])?;
    let file = ardana.models_toml()?;
    let names: Vec<&str> = file["model"]
        .as_array()
        .context("[[model]]")?
        .iter()
        .filter_map(|m| m.get("name").and_then(toml::Value::as_str))
        .collect();
    assert_eq!(names, ["qwen3.5-0.8b"]);
    for path in [&weights, &tokenizer, &config] {
        assert!(path.is_file(), "rm left {} in the HF cache", path.display());
    }
    assert!(!ardana.ok(["list"])?.contains("decider-2b"));
    // Removed, decider-2b is a library model again: `show` says how to get it.
    assert!(
        ardana
            .ok(["show", "decider-2b"])?
            .contains("not pulled yet; `ardana pull decider-2b`")
    );

    for (args, name) in [
        (["show", "nope"], "nope"),
        (["rm", "decider-2b"], "decider-2b"),
    ] {
        let output = ardana.output(args)?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?}");
        assert!(
            stderr.contains(&format!(
                "no model named \"{name}\"; pulled: qwen3.5-0.8b; library, pulled on first use: decider-2b, \
                 decider-4b, qwen3.5-0.8b, smollm3-3b"
            )),
            "{stderr}"
        );
    }
    Ok(())
}

/// Missing input is answered with what to type, before any model loads.
#[test]
fn guidance_without_a_model() -> Result<()> {
    let ardana = Ardana::new("cli-guidance")?;
    let failure = |args: &[&str], stdin: &str| -> Result<String> {
        let mut cmd = ardana.command(args);
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn()?;
        std::io::Write::write_all(child.stdin.as_mut().context("stdin")?, stdin.as_bytes())?;
        let output = child.wait_with_output()?;
        assert_eq!(output.status.code(), Some(1), "{args:?} fails");
        assert!(output.stdout.is_empty(), "{args:?} prints no answer");
        Ok(String::from_utf8(output.stderr)?)
    };
    let no_question = failure(&["run", "decider-2b", "My card was charged twice."], "")?;
    assert!(
        no_question.contains(
            "ask at least one question with --noul, --choice or --score, for example:\n  ardana run decider-2b"
        ),
        "{no_question}"
    );
    let no_state = failure(&["run", "decider-2b", "--noul", "Refund?"], "\n")?;
    assert!(no_state.contains("stdin held no state"), "{no_state}");
    // Nothing listens on the discard port.
    let no_server = failure(&["ps", "--port", "9"], "")?;
    assert!(
        no_server.contains(
            "no ardana serve answers at http://127.0.0.1:9; start one with `ardana serve`"
        ),
        "{no_server}"
    );

    let empty = ardana.output(["list"])?;
    assert_eq!(
        String::from_utf8(empty.stdout)?,
        "NAME  SIZE  PULLED  SOURCE\n"
    );
    assert!(
        String::from_utf8(empty.stderr)?
            .contains("no models pulled yet; try `ardana pull decider-2b`")
    );
    let library = ardana.ok(["show", "decider-4b"])?;
    assert!(library.contains("2.7 GB to download"), "{library}");
    for help in [["--help"], ["run --help"], ["pull --help"]].map(|a| a[0]) {
        let args: Vec<&str> = help.split(' ').collect();
        assert!(ardana.ok(&args)?.contains("Examples:"), "ardana {help}");
    }
    Ok(())
}

/// `ardana run` with its questions on the command line and the state piped in, on decider-2b: each answer under its
/// question, the answer marked, and `--json` the same answers as `q1`, `q2`, `q3` in the order asked.
#[test]
#[ignore = "e2e: decider-2b Q4_K_M GGUF and files in tmp/hf (cargo xtask fetch)"]
fn run_inline_questions() -> Result<()> {
    let ardana = Ardana::new("cli-run-inline")?;
    ardana.ok(["pull", "hf.co/Mapika/decider-2b-GGUF:Q4_K_M"])?;
    let args = [
        "run",
        "decider-2b",
        "--choice",
        "Which team should handle this?",
        "billing",
        "technical",
        "sales",
        "--noul",
        "Does the customer ask for a refund?",
        "--score",
        "How upset is the customer?",
        "calm",
        "annoyed",
        "furious",
    ];
    let ask = |extra: &[&str]| -> Result<String> {
        let mut cmd = ardana.command(args.iter().chain(extra));
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn()?;
        std::io::Write::write_all(
            child.stdin.as_mut().context("stdin")?,
            b"I was charged twice for order A-104 and the app crashes on login. Refund me now!\n",
        )?;
        let output = child.wait_with_output()?;
        println!("{cmd:?}:\n{}", String::from_utf8_lossy(&output.stdout));
        ensure!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?)
    };
    let text = ask(&[])?;
    let blocks: Vec<&str> = text.split("\n\n").collect();
    assert_eq!(blocks.len(), 3, "{text}");
    for (block, title) in blocks.iter().zip([
        "Which team should handle this?",
        "Does the customer ask for a refund?",
        "How upset is the customer?",
    ]) {
        assert!(block.starts_with(title), "{block}");
    }
    assert_eq!(
        blocks[0].matches("\n  * ").count(),
        1,
        "one answer: {}",
        blocks[0]
    );
    assert!(blocks[1].contains("probability of yes"), "{}", blocks[1]);
    assert!(blocks[2].contains("score "), "{}", blocks[2]);

    let json: serde_json::Value = serde_json::from_str(&ask(&["--json"])?)?;
    let kinds: Vec<(&str, &str)> = json["answers"]
        .as_object()
        .context("answers")?
        .iter()
        .map(|(id, a)| (id.as_str(), a["type"].as_str().unwrap_or_default()))
        .collect();
    assert_eq!(kinds, [("q1", "choice"), ("q2", "noul"), ("q3", "score")]);
    Ok(())
}

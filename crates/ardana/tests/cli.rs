//! R2.6 and R2.7: the registry commands `list`, `show` and `rm` under canonical names, library names as spellings of
//! them, and help and error texts that name no size of the library; `ardana run` on an Ollama blob llama.cpp cannot
//! load; the commands' guidance when input is missing, and `ardana run`'s inline questions on a real model.

mod common;

use std::path::Path;

use anyhow::{Context, Result, ensure};
use common::{Ardana, LibraryServer};

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
            "hf.co/Qwen/Qwen3.5-0.8B",
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

/// The rows of an `ardana list`, each cell trimmed.
fn rows(list: &str) -> Vec<Vec<&str>> {
    list.lines()
        .map(|l| {
            l.split("  ")
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .collect()
        })
        .collect()
}

/// R2.6: `pull decider` records `decider:2b`; `list` and `show` print the canonical name, `show` of another spelling
/// shows the same entry, and `show decider:2b-q8_0`, not pulled, prints that name, its file's reference and its bytes;
/// `rm` takes a name as `list` prints it.
#[test]
#[ignore = "e2e: decider-2b and Qwen3.5-0.8B GGUFs and tokenizers in tmp/hf (cargo xtask fetch)"]
fn list_show_rm() -> Result<()> {
    let ardana = Ardana::new("cli-list-show-rm")?;
    let empty = ardana.ok(["list"])?;
    assert_eq!(empty.trim(), "NAME  SIZE  PULLED  SOURCE");

    let pulled = ardana.ok(["pull", "decider"])?;
    assert_eq!(
        pulled.trim(),
        "pulled decider:2b (hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf, 1.3 GB)"
    );
    ardana.ok(["pull", "qwen3.5"])?;

    let list = ardana.ok(["list"])?;
    assert_eq!(
        rows(&list),
        [
            vec!["NAME", "SIZE", "PULLED", "SOURCE"],
            vec![
                "decider:2b",
                "1.3 GB",
                "today",
                "hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf"
            ],
            vec![
                "qwen3.5:0.8b",
                "563.0 MB",
                "today",
                "hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Qwen3.5-0.8B-Q4_0.gguf"
            ],
        ],
        "{list}"
    );

    let show: serde_json::Value =
        serde_json::from_str(&ardana.ok(["show", "decider:2b", "--json"])?)?;
    let shown = ardana.ok(["show", "decider:2b"])?;
    for line in [
        "    name            decider:2b",
        "    profile         decider-2b-v11",
        "    layout          plain (decider format)",
        "    choice          temperature 1.164",
    ] {
        assert!(shown.contains(line), "{line:?} in:\n{shown}");
    }
    assert_eq!(ardana.ok(["show", "Decider:LATEST"])?, shown);
    let weights = common::hf_file("Mapika/decider-2b-GGUF", "decider-2b-v11-Q4_K_M.gguf")?;
    assert_eq!(show["name"], "decider:2b");
    assert_eq!(
        show["source"],
        "hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf"
    );
    assert_eq!(show["weights"], weights.to_string_lossy().as_ref());
    assert_eq!(show["runtime"], "llama.cpp");
    assert_eq!(show["profile"]["name"], "decider-2b-v11");
    assert_eq!(show["profile"]["layout"]["kind"], "plain");
    assert_eq!(show["profile"]["temperature_by_type"]["choice"], 1.164);
    assert!(show["pulled_at"].is_string());

    // Another quant, not pulled: its canonical name, the reference of its file and its bytes.
    let other = ardana.ok(["show", "decider:2b-Q8_0"])?;
    for line in [
        "    name        decider:2b-q8_0",
        "    source      hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q8_0.gguf",
        "    size        2.0 GB to download",
        "    status      not pulled yet; `ardana pull decider:2b-q8_0` or its first run downloads it",
    ] {
        assert!(other.contains(line), "{line:?} in:\n{other}");
    }

    let tokenizer = common::hf_file("Mapika/decider-2b-GGUF", "tokenizer.json")?;
    let config = common::hf_file("Mapika/decider-2b-GGUF", "decider_config.json")?;
    // Q25: `rm` takes the name `list` prints, and no other spelling.
    let spelled = ardana.output(["rm", "decider"])?;
    assert!(!spelled.status.success());
    ardana.ok(["rm", "decider:2b"])?;
    let file = ardana.models_toml()?;
    let names: Vec<&str> = file["model"]
        .as_array()
        .context("[[model]]")?
        .iter()
        .filter_map(|m| m.get("name").and_then(toml::Value::as_str))
        .collect();
    assert_eq!(names, ["qwen3.5:0.8b"]);
    for path in [&weights, &tokenizer, &config] {
        assert!(path.is_file(), "rm left {} in the HF cache", path.display());
    }
    assert!(!ardana.ok(["list"])?.contains("decider:2b"));
    // Removed, decider:2b is a library model again: `show` says how to get it.
    assert!(
        ardana
            .ok(["show", "decider"])?
            .contains("not pulled yet; `ardana pull decider:2b`")
    );

    for (args, name) in [
        (["show", "nope"], "nope"),
        (["rm", "decider:2b"], "decider:2b"),
        (["rm", "decider"], "decider"),
    ] {
        let output = ardana.output(args)?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?}");
        assert!(
            stderr.contains(&format!(
                "no model named \"{name}\"; pulled: qwen3.5:0.8b; the library at https://ardana.ai/models/ is \
                 pulled on first use"
            )),
            "{stderr}"
        );
    }
    Ok(())
}

/// R2.6: every spelling of a size means its canonical name (Q10): under a URL source `pull decider` records
/// `decider:2b` from the family's manifest and `run decider` then sends nothing and runs it; a tag the family does not
/// list fails with Q12's text and pulls nothing; a size without a `decider_config.json` gets the stock profile named
/// canonically (Q17).
#[test]
#[ignore = "e2e: decider-2b Q4_K_M and Qwen3.5-0.8B in tmp/hf (cargo xtask fetch), loaded through llama.cpp"]
fn library_names_are_spellings() -> Result<()> {
    let ardana = Ardana::new("cli-library-spellings")?;
    let library = LibraryServer::start(&common::snapshot_bytes()?)?;
    let run = |args: &[&str]| -> Result<std::process::Output> {
        let mut cmd = ardana.command(args);
        cmd.env("ARDANA_LIBRARY", &library.url);
        let output = cmd.output()?;
        println!(
            "{cmd:?} ({}):\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output)
    };
    let paths = || -> Vec<String> { library.requests().into_iter().map(|r| r.path).collect() };

    let out = run(&["pull", "decider"])?;
    assert!(out.status.success());
    assert_eq!(paths(), ["/models/decider.json"]);
    let entry = ardana.entry("decider:2b")?;
    assert_eq!(
        entry["source"].as_str(),
        Some("hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf")
    );

    let ticket = common::request("ticket.json");
    let out = run(&[
        "run",
        "decider",
        "--json",
        "--request",
        &ticket.to_string_lossy(),
    ])?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(!stderr.contains("pulling"), "{stderr}");
    common::check_ticket(&serde_json::from_slice(&out.stdout)?)?;
    assert_eq!(paths(), ["/models/decider.json"], "the run sent nothing");

    let out = run(&["pull", "gemma-4:9b"])?;
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(
            "gemma-4 has no tag \"9b\"; its tags are e2b, e2b-q8_0, e2b-bf16, e4b, e4b-q8_0, e4b-bf16, 12b, \
             12b-q8_0, 12b-bf16, 26b-a4b, 26b-a4b-q8_0, 26b-a4b-bf16, 31b, 31b-q8_0, 31b-bf16"
        ),
        "{stderr}"
    );
    assert_eq!(paths(), ["/models/decider.json", "/models/gemma-4.json"]);

    let out = run(&["pull", "qwen3.5:0.8b"])?;
    assert!(out.status.success());
    let qwen = ardana.entry("qwen3.5:0.8b")?;
    let profile = qwen["profile"].as_table().context("profile")?;
    assert_eq!(profile["name"].as_str(), Some("qwen3.5:0.8b"));
    assert_eq!(profile["layout"]["kind"].as_str(), Some("chat"));
    let names: Vec<String> = ardana.models_toml()?["model"]
        .as_array()
        .context("[[model]]")?
        .iter()
        .filter_map(|m| {
            m.get("name")
                .and_then(toml::Value::as_str)
                .map(String::from)
        })
        .collect();
    assert_eq!(names, ["decider:2b", "qwen3.5:0.8b"]);
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
    let no_question = failure(&["run", "decider", "My card was charged twice."], "")?;
    assert!(
        no_question.contains(
            "ask at least one question with --noul, --choice or --score, for example:\n  ardana run decider "
        ),
        "{no_question}"
    );
    let no_state = failure(&["run", "decider", "--noul", "Refund?"], "\n")?;
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
            .contains("no models pulled yet; try `ardana pull decider`")
    );
    let library = ardana.ok(["show", "decider:4b"])?;
    assert!(library.contains("2.7 GB to download"), "{library}");
    for help in [["--help"], ["run --help"], ["pull --help"]].map(|a| a[0]) {
        let args: Vec<&str> = help.split(' ').collect();
        assert!(ardana.ok(&args)?.contains("Examples:"), "ardana {help}");
    }
    Ok(())
}

/// `ardana run` with its questions on the command line and the state piped in, on decider:2b: each answer under its
/// question, the answer marked, and `--json` the same answers as `q1`, `q2`, `q3` in the order asked.
#[test]
#[ignore = "e2e: decider-2b Q4_K_M GGUF and files in tmp/hf (cargo xtask fetch)"]
fn run_inline_questions() -> Result<()> {
    let ardana = Ardana::new("cli-run-inline")?;
    ardana.ok(["pull", "decider"])?;
    let args = [
        "run",
        "decider:2b",
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

/// The names of `names` that `text` holds as words of their own (not `decider:2b` inside `decider:2b-q8_0`).
fn named_in<'a>(text: &str, names: &'a [String]) -> Vec<&'a str> {
    let part = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':');
    names
        .iter()
        .filter(|name| {
            text.match_indices(name.as_str()).any(|(at, _)| {
                let before = text[..at].chars().next_back().is_none_or(|c| !part(c));
                let after = text[at + name.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !part(c));
                before && after
            })
        })
        .map(String::as_str)
        .collect()
}

/// Every `--help` of `ardana`.
const HELPS: [&[&str]; 8] = [
    &["--help"],
    &["pull", "--help"],
    &["run", "--help"],
    &["show", "--help"],
    &["list", "--help"],
    &["rm", "--help"],
    &["ps", "--help"],
    &["serve", "--help"],
];

/// R2.7: no model list is compiled into `ardana`: the built binary holds no repository of the snapshot, neither a
/// size's GGUF repository nor a browser variant's, and no size's canonical name; help names the family `decider`,
/// writes any other model as `<family>:<size>-<quant>` and an entry to remove as `ardana rm <name>`, and names no size
/// and no other family.
#[test]
fn the_binary_holds_no_library() -> Result<()> {
    let binary =
        String::from_utf8_lossy(&std::fs::read(env!("CARGO_BIN_EXE_ardana"))?).into_owned();
    let models = common::snapshot()?;
    let families = models["models"].as_array().context("models")?;
    let mut checked = 0;
    for family in families {
        for size in family["sizes"].as_array().context("sizes")? {
            let references = [&size["gguf"]["repo"], &size["browser"]["repo"]];
            for repo in references.iter().filter_map(|r| r.as_str()) {
                let repo = repo.strip_prefix("hf.co/").unwrap_or(repo);
                assert!(
                    !binary.contains(repo),
                    "{} holds {repo}",
                    env!("CARGO_BIN_EXE_ardana")
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 15, "{checked} references checked");
    let sizes = common::snapshot_names()?;
    assert_eq!(sizes.len(), 12);
    assert_eq!(named_in(&binary, &sizes), [] as [&str; 0]);

    let ardana = Ardana::new("cli-binary-holds-no-library")?;
    let others: Vec<String> = families
        .iter()
        .filter_map(|f| f["name"].as_str())
        .filter(|name| *name != "decider")
        .map(String::from)
        .collect();
    for args in HELPS {
        let text = ardana.ok(args)?;
        assert_eq!(
            named_in(&text, &sizes),
            [] as [&str; 0],
            "{args:?}:\n{text}"
        );
        assert_eq!(
            named_in(&text, &others),
            [] as [&str; 0],
            "{args:?}:\n{text}"
        );
        let wanted: &[&str] = match args[0] {
            "--help" | "pull" | "show" => &["decider", "<family>:<size>-<quant>"],
            "run" => &["ardana run decider "],
            "rm" => &["ardana rm <name>"],
            _ => &[],
        };
        for wanted in wanted {
            assert!(text.contains(wanted), "{wanted:?} in {args:?}:\n{text}");
        }
    }
    Ok(())
}

/// With no server at the library's URL and nothing cached, `pull` and `run` of a library model exit non-zero with an
/// error naming the family's manifest URL and the cause, and no panic.
#[test]
fn an_unreachable_library_is_an_error() -> Result<()> {
    let ardana = Ardana::new("cli-unreachable-library")?;
    for args in [
        vec!["pull", "decider:2b"],
        vec!["run", "decider:2b", "A state.", "--noul", "Is it?"],
    ] {
        let mut cmd = ardana.command(&args);
        cmd.env("ARDANA_LIBRARY", "http://127.0.0.1:9/models.json");
        let output = cmd.output()?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("{cmd:?} ({}):\n{stderr}", output.status);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?} prints no answer");
        assert!(
            stderr.contains("reading the library at http://127.0.0.1:9/models/decider.json: "),
            "{stderr}"
        );
        assert!(stderr.to_lowercase().contains("refused"), "{stderr}");
        assert!(!stderr.contains("panicked"), "{stderr}");
    }
    assert!(!ardana.home.join("library").exists(), "nothing cached");
    Ok(())
}

/// Under a URL source, `list`, `show`, `rm`, `ps` and every `--help` send no request; `show` of a name not pulled
/// answers from the family's cached manifest; help and unknown-model texts name no size of the library and carry
/// `https://ardana.ai/models/`.
#[test]
fn offline_commands_never_fetch() -> Result<()> {
    let ardana = Ardana::new("cli-offline-commands")?;
    // A listener at the library's URL that counts what reaches it.
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let url = format!(
        "http://127.0.0.1:{}/models.json",
        listener.local_addr()?.port()
    );
    let connections = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = connections.clone();
    std::thread::spawn(move || {
        for _ in listener.incoming() {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });
    // The cache holds the decider family's manifest, as an earlier pull would have left it.
    let snapshot = common::snapshot()?;
    let family = snapshot["models"]
        .as_array()
        .context("models")?
        .iter()
        .find(|m| m["name"] == "decider")
        .context("decider")?;
    let cached = ardana.home.join("library/models/decider.json");
    std::fs::create_dir_all(cached.parent().context("models")?)?;
    std::fs::write(
        &cached,
        serde_json::json!({"schema": 1, "model": family}).to_string(),
    )?;
    let names = common::snapshot_names()?;
    let page = "https://ardana.ai/models/";
    let run = |args: &[&str]| -> Result<(Option<i32>, String, String)> {
        let mut cmd = ardana.command(args);
        cmd.env("ARDANA_LIBRARY", &url);
        let output = cmd.output()?;
        let stdout = String::from_utf8(output.stdout)?;
        let stderr = String::from_utf8(output.stderr)?;
        println!("{cmd:?} ({}):\n{stdout}{stderr}", output.status);
        Ok((output.status.code(), stdout, stderr))
    };

    let (code, stdout, stderr) = run(&["list"])?;
    assert_eq!(
        (code, stdout.as_str()),
        (Some(0), "NAME  SIZE  PULLED  SOURCE\n")
    );
    assert!(
        stderr.contains("no models pulled yet; try `ardana pull decider`") && stderr.contains(page),
        "{stderr}"
    );
    let (code, stdout, _) = run(&["show", "decider:2b"])?;
    assert_eq!(code, Some(0));
    assert!(
        stdout.contains("1.3 GB to download")
            && stdout
                .contains("not pulled yet; `ardana pull decider:2b` or its first run downloads it"),
        "{stdout}"
    );
    let (code, stdout, _) = run(&["show", "Decider:2B-Q4_K_M", "--json"])?;
    assert_eq!(code, Some(0));
    let shown: serde_json::Value = serde_json::from_str(&stdout)?;
    assert_eq!(shown["name"], "decider:2b");
    assert_eq!(
        shown["source"],
        "hf.co/Mapika/decider-2b-GGUF:decider-2b-v11-Q4_K_M.gguf"
    );
    assert_eq!(shown["size"], 1_274_396_800_u64);
    assert_eq!(shown["pulled"], false);
    // A name the cache does not hold is unknown here, without a request; so is a name to remove.
    let unknown = |name: &str| {
        format!(
            "no model named \"{name}\"; none pulled yet; the library at {page} is pulled on first use"
        )
    };
    for (args, name) in [
        (["show", "gemma-4:e2b"], "gemma-4:e2b"),
        (["rm", "nope"], "nope"),
        (["rm", "decider:2b"], "decider:2b"),
    ] {
        let (code, stdout, stderr) = run(&args)?;
        assert_eq!((code, stdout.as_str()), (Some(1), ""), "{args:?}");
        assert!(stderr.contains(&unknown(name)), "{args:?}: {stderr}");
        // The error echoes the name asked for and names no other size of the library.
        let unasked: Vec<String> = names.iter().filter(|n| *n != name).cloned().collect();
        assert_eq!(named_in(&stderr, &unasked), [] as [&str; 0], "{stderr}");
    }
    let (code, _, stderr) = run(&["ps", "--port", "9"])?;
    assert_eq!(code, Some(1));
    assert!(stderr.contains("no ardana serve answers"), "{stderr}");
    for help in HELPS {
        let (code, stdout, _) = run(help)?;
        assert_eq!(code, Some(0), "{help:?}");
        assert_eq!(
            named_in(&stdout, &names),
            [] as [&str; 0],
            "{help:?}:\n{stdout}"
        );
        if help.len() == 1 || matches!(help[0], "pull" | "run") {
            assert!(stdout.contains(page), "{help:?}:\n{stdout}");
        }
    }
    assert!(run(&["--help"])?.1.contains("ARDANA_LIBRARY"));
    assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 0);
    Ok(())
}

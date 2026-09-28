//! R4.4 and R4.6: the registry commands `list`, `show` and `rm`, and `ardana run` on an Ollama blob llama.cpp cannot
//! load.

mod common;

use std::path::Path;

use anyhow::{Context, Result};
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
    assert_eq!(empty.trim(), "NAME  SOURCE  FORMAT  SIZE  LAYOUT");

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
            vec!["NAME", "SOURCE", "FORMAT", "SIZE", "LAYOUT"],
            vec![
                "decider-2b",
                "hf.co/Mapika/decider-2b-GGUF:Q4_K_M",
                "gguf",
                "1.3 GB",
                "plain"
            ],
            vec![
                "qwen3.5-0.8b",
                "hf.co/ggml-org/Qwen3.5-0.8B-GGUF:Q4_0",
                "gguf",
                "563.0 MB",
                "chat"
            ],
        ],
        "{list}"
    );

    let show: serde_json::Value = serde_json::from_str(&ardana.ok(["show", "decider-2b"])?)?;
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

    for args in [["show", "decider-2b"], ["rm", "decider-2b"]] {
        let output = ardana.output(args)?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?}");
        assert!(
            stderr.contains("no model named \"decider-2b\"; available: qwen3.5-0.8b"),
            "{stderr}"
        );
    }
    Ok(())
}

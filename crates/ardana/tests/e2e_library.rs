//! The model library end to end, offline from the files `cargo xtask fetch` put into `tmp/hf`: `ardana pull <name>`,
//! `ardana run <name>` and `ardana serve` pulling a library model on first use (the default one for `jev-latest` and
//! requests without a model), and `serve` picking up a model `ardana pull` adds while it runs.

mod common;

use anyhow::{Context, Result};
use common::{Ardana, Served};
use serde_json::{Value, json};

/// `HF_HUB_OFFLINE=1` (set by [`Ardana`]) with an unreachable endpoint: nothing may call the Hub.
const NO_HUB: (&str, &str) = ("HF_ENDPOINT", "http://127.0.0.1:9");

fn request(name: &str) -> Result<Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(
        common::request(name),
    )?)?)
}

#[test]
#[ignore = "e2e: decider-2b Q4_K_M, Qwen3.5-0.8B and SmolLM3-3B GGUFs and tokenizers in tmp/hf (cargo xtask fetch)"]
fn pull_library_names() -> Result<()> {
    let ardana = Ardana::new("e2e-library-pull")?;
    let pull = |name: &str| {
        let mut cmd = ardana.command(["pull", name]);
        cmd.env(NO_HUB.0, NO_HUB.1);
        cmd.output()
    };

    let out = pull("decider-2b")?;
    assert!(out.status.success(), "{out:?}");
    let decider = ardana.entry("decider-2b")?;
    assert_eq!(
        decider["source"].as_str(),
        Some("hf.co/Mapika/decider-2b-GGUF:Q4_K_M")
    );
    assert_eq!(
        decider["weights"].as_str().map(std::path::PathBuf::from),
        Some(common::hf_file(
            "Mapika/decider-2b-GGUF",
            "decider-2b-v11-Q4_K_M.gguf"
        )?)
    );
    let profile = decider["profile"].as_table().context("profile")?;
    assert_eq!(
        profile["name"].as_str(),
        Some("decider-2b-v11"),
        "decider_config.json"
    );
    assert_eq!(profile["layout"]["kind"].as_str(), Some("plain"));

    // `name:quant` pulls another GGUF of the same repository; offline, only the fetched Q4_K_M is there.
    let out = pull("decider-2b:q8_0")?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains("hf.co/Mapika/decider-2b-GGUF: no GGUF for the quant q8_0"),
        "{stderr}"
    );

    for (name, gguf, tokenizer) in [
        (
            "qwen3.5-0.8b",
            ("ggml-org/Qwen3.5-0.8B-GGUF", "Qwen3.5-0.8B-Q4_0.gguf"),
            "Qwen/Qwen3.5-0.8B",
        ),
        (
            "smollm3-3b",
            ("ggml-org/SmolLM3-3B-GGUF", "SmolLM3-Q4_K_M.gguf"),
            "HuggingFaceTB/SmolLM3-3B",
        ),
    ] {
        let out = pull(name)?;
        assert!(out.status.success(), "{name}: {out:?}");
        let entry = ardana.entry(name)?;
        assert_eq!(
            entry["weights"].as_str().map(std::path::PathBuf::from),
            Some(common::hf_file(gguf.0, gguf.1)?)
        );
        assert_eq!(
            entry["tokenizer"].as_str().map(std::path::PathBuf::from),
            Some(common::hf_file(tokenizer, "tokenizer.json")?)
        );
        let profile = entry["profile"].as_table().context("profile")?;
        assert_eq!(profile["name"].as_str(), Some(name));
        assert_eq!(profile["layout"]["kind"].as_str(), Some("chat"));
        assert!(
            profile["release_date"].as_str().is_some(),
            "the library's release date"
        );
    }
    let file = ardana.models_toml()?;
    let names: Vec<&str> = file["model"]
        .as_array()
        .context("[[model]]")?
        .iter()
        .filter_map(|m| m.get("name").and_then(toml::Value::as_str))
        .collect();
    assert_eq!(names, ["decider-2b", "qwen3.5-0.8b", "smollm3-3b"]);
    Ok(())
}

#[test]
#[ignore = "e2e: decider-2b Q4_K_M GGUF in tmp/hf (cargo xtask fetch), loaded through llama.cpp"]
fn run_pulls_a_library_model_first() -> Result<()> {
    let ardana = Ardana::new("e2e-library-run")?;
    let mut cmd = ardana.command(["run", "decider-2b", "--request"]);
    cmd.arg(common::request("ticket.json"))
        .env(NO_HUB.0, NO_HUB.1);
    let out = cmd.output()?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    println!("{stderr}");
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains("pulling decider-2b (hf.co/Mapika/decider-2b-GGUF:Q4_K_M, 1.3 GB)"),
        "{stderr}"
    );
    common::check_ticket(&serde_json::from_slice(&out.stdout)?)?;
    assert_eq!(
        ardana.entry("decider-2b")?["name"].as_str(),
        Some("decider-2b")
    );

    // Pulled now: the second run pulls nothing.
    let mut cmd = ardana.command(["run", "decider-2b", "--request"]);
    cmd.arg(common::request("ticket.json"))
        .env(NO_HUB.0, NO_HUB.1);
    let out = cmd.output()?;
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("pulling"));
    Ok(())
}

#[test]
#[ignore = "e2e: decider-2b Q4_K_M GGUF in tmp/hf (cargo xtask fetch), loaded through llama.cpp"]
fn serve_pulls_the_default_model_on_first_use() -> Result<()> {
    let ardana = Ardana::new("e2e-library-serve")?;
    let server = Served::start(&ardana, &[], &[NO_HUB])?;
    let ticket = request("ticket.json")?;

    // `jev-latest` and `decider-2b` at once: one pull, both answered by decider-2b.
    let answers: Vec<(u16, Value)> = std::thread::scope(|scope| {
        let asks: Vec<_> = [json!("jev-latest"), json!("decider-2b")]
            .into_iter()
            .map(|model| {
                let mut body = ticket.clone();
                body["model"] = model;
                let server = &server;
                scope.spawn(move || server.decide(&body))
            })
            .collect();
        asks.into_iter()
            .map(|ask| ask.join().expect("the request thread ends"))
            .collect::<Result<_>>()
    })?;
    for (status, body) in &answers {
        assert_eq!(*status, 200, "{body}");
        common::check_ticket(body)?;
    }
    let log = server.log_text();
    assert_eq!(
        log.matches(
            "ardana serve: pulling decider-2b (hf.co/Mapika/decider-2b-GGUF:Q4_K_M, 1.3 GB)"
        )
        .count(),
        1,
        "one pull for both requests:\n{log}"
    );
    assert_eq!(
        ardana.entry("decider-2b")?["source"].as_str(),
        Some("hf.co/Mapika/decider-2b-GGUF:Q4_K_M")
    );

    // A request without a model, now that decider-2b is pulled.
    let mut bare = ticket.clone();
    bare.as_object_mut().context("request")?.remove("model");
    let (status, body) = server.decide(&bare)?;
    assert_eq!(status, 200, "{body}");
    common::check_ticket(&body)?;

    let listed = server.models()?;
    assert_eq!(listed["models"][0]["name"], "decider-2b");
    assert_eq!(listed["models"][0]["x_pulled"], true);
    assert_eq!(listed["models"][0]["x_default"], true);
    Ok(())
}

#[test]
#[ignore = "e2e: Qwen3.5-0.8B GGUF and tokenizer in tmp/hf (cargo xtask fetch), loaded through llama.cpp"]
fn serve_picks_up_models_pulled_while_it_runs() -> Result<()> {
    let ardana = Ardana::new("e2e-library-serve-live")?;
    let server = Served::start(&ardana, &[], &[NO_HUB])?;
    let pulled = |listed: &Value| -> Vec<String> {
        listed["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["x_pulled"] == true)
            .filter_map(|m| m["name"].as_str().map(String::from))
            .collect()
    };
    assert!(pulled(&server.models()?).is_empty());

    let mut cmd = ardana.command(["pull", "qwen3.5-0.8b"]);
    cmd.env(NO_HUB.0, NO_HUB.1);
    assert!(cmd.status()?.success(), "{cmd:?}");
    let listed = server.models()?;
    assert_eq!(pulled(&listed), ["qwen3.5-0.8b"]);
    assert_eq!(listed["models"][0]["x_default"], true, "{listed}");

    let mut sentiment = request("sentiment.json")?;
    sentiment["model"] = json!("qwen3.5-0.8b");
    let (status, body) = server.decide(&sentiment)?;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["model"], "qwen3.5-0.8b");
    common::check_sentiment(&body)?;
    assert!(
        !server.log_text().contains("pulling"),
        "nothing was pulled by the server"
    );

    // A name that is neither pulled nor in the library lists both.
    sentiment["model"] = json!("nope");
    let (status, body) = server.decide(&sentiment)?;
    assert_eq!(status, 404, "{body}");
    assert_eq!(
        body["detail"]["message"],
        "no model named \"nope\"; pulled: qwen3.5-0.8b; library, pulled on first use: decider-2b, decider-4b, \
         qwen3.5-0.8b, smollm3-3b"
    );
    Ok(())
}

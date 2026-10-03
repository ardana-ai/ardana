//! R5.8 on the binary: `ardana serve` binds 127.0.0.1 and takes its key from `ARDANA_API_KEY`; with nothing pulled it
//! lists the library models it pulls on first use. R3.1: `ARDANA_PUBLIC` makes it a public server, which runs no model
//! and reads no registry.

mod common;

use anyhow::{Context, Result};
use common::{Ardana, Served};
use serde_json::{Value, json};

#[test]
fn serve_binds_localhost_and_reads_the_key_from_the_environment() -> Result<()> {
    let ardana = Ardana::new("serve-api-key-env")?;
    let server = Served::start(&ardana, &[], &[("ARDANA_API_KEY", "serve-test-key")])?;
    let banner = server.log_text();
    assert!(
        banner.contains(&format!("listening on http://127.0.0.1:{} ", server.port))
            && banner.contains("no models pulled yet; the first request pulls decider-2b")
            && banner.contains("API key required"),
        "{banner}"
    );

    assert_eq!(server.get("/health", "")?.0, 200);
    let (status, body) = server.get("/v1/models", "")?;
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("authentication_error"), "{body}");
    assert_eq!(
        server
            .get("/v1/models", "Authorization: Bearer wrong\r\n")?
            .0,
        401
    );
    let (status, body) = server.get("/v1/models", "Authorization: Bearer serve-test-key\r\n")?;
    assert_eq!(status, 200, "{body}");
    let listed: Value = serde_json::from_str(&body)?;
    let models = listed["models"].as_array().context("models")?;
    let names: Vec<&str> = models.iter().filter_map(|m| m["name"].as_str()).collect();
    assert_eq!(
        names,
        [
            "decider-2b",
            "decider-0.8b",
            "decider-4b",
            "qwen3.5-0.8b",
            "smollm3-3b"
        ]
    );
    assert!(models.iter().all(|m| m["x_pulled"] == false), "{body}");
    assert_eq!(models[0]["x_default"], true, "{body}");
    Ok(())
}

#[test]
fn serve_public_reads_the_flag_from_the_environment() -> Result<()> {
    let ardana = Ardana::new("serve-public-env")?;
    let server = Served::start(&ardana, &[], &[("ARDANA_PUBLIC", "1")])?;
    let banner = server.log_text();
    assert!(
        banner
            .contains("(public: browser models run in the visitors' tabs, and no model runs here)"),
        "{banner}"
    );

    let models = server.models()?;
    let models = models["models"].as_array().context("models")?;
    assert_eq!(models.len(), 5, "{models:?}");
    assert!(
        models
            .iter()
            .all(|m| m["x_pulled"] == false && m.get("x_default").is_none()),
        "{models:?}"
    );
    let browser_default: Vec<&Value> = models
        .iter()
        .filter(|m| m["x_browser_default"] == true)
        .map(|m| &m["name"])
        .collect();
    assert_eq!(browser_default, [&json!("decider-0.8b")]);

    let (status, body) =
        server.decide(&json!({"model": "decider-2b", "state": "s", "questions": {}}))?;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["detail"]["error_type"], "permission_error");
    assert_eq!(
        server.get("/health", "")?,
        (200, r#"{"status":"ok"}"#.into())
    );
    assert!(
        !ardana.home.exists(),
        "a public server writes nothing to its Ardana home"
    );
    Ok(())
}

/// A public server has no registry access: a `models.toml` no local server could read does not stop it, and it is left
/// as it was.
#[test]
fn serve_public_reads_no_registry() -> Result<()> {
    let ardana = Ardana::new("serve-public-registry")?;
    std::fs::create_dir_all(&ardana.home)?;
    let file = ardana.home.join("models.toml");
    let corrupt = "[[model]]\nname = \"decider-2b\"\nweights = \n";
    std::fs::write(&file, corrupt)?;
    let written = std::fs::metadata(&file)?.modified()?;

    let server = Served::start(&ardana, &["--public"], &[])?;
    let models = server.models()?;
    let names: Vec<&str> = models["models"]
        .as_array()
        .context("models")?
        .iter()
        .filter_map(|m| m["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "decider-2b",
            "decider-0.8b",
            "decider-4b",
            "qwen3.5-0.8b",
            "smollm3-3b"
        ]
    );
    assert_eq!(server.get("/health", "")?.0, 200);
    drop(server);

    assert_eq!(std::fs::read_to_string(&file)?, corrupt);
    assert_eq!(std::fs::metadata(&file)?.modified()?, written);
    let left: Vec<std::path::PathBuf> = std::fs::read_dir(&ardana.home)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    assert_eq!(left, [file], "nothing else is written to the home");
    Ok(())
}

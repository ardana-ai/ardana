//! R5.8 on the binary: `ardana serve` binds 127.0.0.1 and takes its key from `ARDANA_API_KEY`; with nothing pulled it
//! lists the library models it pulls on first use.

mod common;

use anyhow::{Context, Result};
use common::{Ardana, Served};
use serde_json::Value;

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
        ["decider-2b", "decider-4b", "qwen3.5-0.8b", "smollm3-3b"]
    );
    assert!(models.iter().all(|m| m["x_pulled"] == false), "{body}");
    assert_eq!(models[0]["x_default"], true, "{body}");
    Ok(())
}

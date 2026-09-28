//! The embedded playground: `index.html` at `/` and for every unknown non-`/v1` path, its assets from memory, and
//! unknown `/v1/*` paths still answered by the API. The page is the one `build.rs` embedded: the playground's `dist/`
//! (or `ARDANA_PLAYGROUND_DIST`), else the placeholder (R6.4 builds with an empty directory to get it).

mod common;

use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use ardana_server::{ModelOptions, router};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{get, models};
use serde_json::json;
use tower::ServiceExt;

/// The directory `build.rs` embeds, by the same rule.
fn embedded_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dist = std::env::var_os("ARDANA_PLAYGROUND_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../ardana-playground/dist"));
    if dist.join("index.html").is_file() {
        dist
    } else {
        manifest.join("placeholder")
    }
}

/// `GET uri` without asking for compression: status, content type and body.
async fn page(app: &Router, uri: &str) -> Result<(StatusCode, String, Vec<u8>)> {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty())?)
        .await?;
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
    Ok((status, content_type, bytes.to_vec()))
}

#[tokio::test]
async fn serves_the_embedded_page() -> Result<()> {
    let (_, models) = models("playground-page", &[], ModelOptions::default())?;
    let app = router(models, None);
    let dir = embedded_dir();
    let index = std::fs::read(dir.join("index.html"))
        .with_context(|| format!("reading {}", dir.display()))?;

    for uri in ["/", "/index.html", "/some/deep/link", "/share/unknown?x=1"] {
        let (status, content_type, body) = page(&app, uri).await?;
        ensure!(status == StatusCode::OK, "{uri}: {status}");
        ensure!(
            content_type.starts_with("text/html"),
            "{uri}: {content_type}"
        );
        ensure!(body == index, "{uri} is not {}/index.html", dir.display());
    }

    // Every other file of the directory is served under its own name.
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !path.is_file() || name == "index.html" {
            continue;
        }
        let (status, _, _) = page(&app, &format!("/{name}")).await?;
        ensure!(status == StatusCode::OK, "/{name}: {status}");
    }

    let not_found = get(&app, "/v1/nope").await?;
    assert_eq!(not_found.status, 404);
    assert_eq!(not_found.body, json!({"detail": "Not Found"}));
    Ok(())
}

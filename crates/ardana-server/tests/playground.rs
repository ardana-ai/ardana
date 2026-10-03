//! The embedded playground: `index.html` at `/` and for every unknown non-`/v1` path, its assets from memory, and
//! unknown `/v1/*` paths still answered by the API. The page is the one `build.rs` embedded: the playground's `dist/`
//! (or `ARDANA_PLAYGROUND_DIST`), else the placeholder (R6.4 builds with an empty directory to get it).

mod common;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use ardana_server::{ModelOptions, router};
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
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

/// The path of every file under `dir`, as the page names it (`/fonts/onest-latin-wght-normal.woff2`).
fn files(dir: &Path, base: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            files(&path, base, out)?;
        } else if let Ok(relative) = path.strip_prefix(base) {
            let parts: Vec<String> = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(format!("/{}", parts.join("/")));
        }
    }
    Ok(())
}

/// `GET uri` with `headers`: the status and the response's headers.
async fn head(
    app: &Router,
    uri: &str,
    headers: &[(&str, &str)],
) -> Result<(StatusCode, HeaderMap)> {
    let mut request = Request::builder().uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app.clone().oneshot(request.body(Body::empty())?).await?;
    Ok((response.status(), response.headers().clone()))
}

/// Every file of the page goes out with the cache policy its name allows, on a local server and a public one alike:
/// the page and the in-tab engine (`/engine/`: the engine module and `engine.js`), whose names outlive their contents,
/// are revalidated on every load and answer 304 while unchanged; every other file keeps memory-serve's week, its name
/// changing with its contents (or never changing at all, as the fonts and onnxruntime-web's versioned directory). Every
/// response keeps the page cross-origin isolated.
#[tokio::test]
async fn caches_only_what_its_name_pins() -> Result<()> {
    let dir = embedded_dir();
    let mut paths = Vec::new();
    files(&dir, &dir, &mut paths)?;
    paths.sort();
    // The playground's own dist holds the in-tab engine; the placeholder holds none of it.
    if dir.ends_with("dist") {
        for file in [
            "/engine/engine.js",
            "/engine/ardana-engine.js",
            "/engine/ardana-engine_bg.wasm",
        ] {
            ensure!(
                paths.iter().any(|path| path == file),
                "{} lacks {file}",
                dir.display()
            );
        }
    }
    for public in [false, true] {
        let opts = ModelOptions {
            public,
            ..ModelOptions::default()
        };
        let (_, models) = models(&format!("playground-cache-{public}"), &[], opts)?;
        let app = router(models, None);
        for path in &paths {
            let (status, headers) = head(&app, path, &[]).await?;
            ensure!(status == StatusCode::OK, "{path}: {status}");
            let header = |name: &str| {
                headers
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
            };
            let revalidated = path.ends_with(".html") || path.starts_with("/engine/");
            let expected = if revalidated {
                "no-cache"
            } else {
                "max-age=604800, stale-while-revalidate=86400"
            };
            ensure!(
                header("cache-control") == expected,
                "{path}: {:?}",
                header("cache-control")
            );
            for (name, value) in [
                ("cross-origin-opener-policy", "same-origin"),
                ("cross-origin-embedder-policy", "require-corp"),
                ("cross-origin-resource-policy", "same-origin"),
            ] {
                ensure!(header(name) == value, "{path}: {name} {:?}", header(name));
            }
            if revalidated {
                let etag = header("etag").to_string();
                ensure!(!etag.is_empty(), "{path} has no ETag");
                let (status, again) = head(&app, path, &[("if-none-match", &etag)]).await?;
                ensure!(
                    status == StatusCode::NOT_MODIFIED,
                    "{path} revalidated: {status}"
                );
                ensure!(
                    again.get("cache-control").and_then(|v| v.to_str().ok()) == Some("no-cache"),
                    "{path} revalidated: {again:?}"
                );
            }
        }
    }
    Ok(())
}

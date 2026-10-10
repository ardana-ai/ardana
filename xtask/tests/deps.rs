use std::path::Path;

use serde_json::{Value, json};
use xtask::deps::{check_deps, metadata};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn package<'a>(metadata: &'a mut Value, name: &str) -> &'a mut Value {
    let packages = metadata["packages"].as_array_mut().unwrap();
    packages.iter_mut().find(|p| p["name"] == name).unwrap()
}

fn add_edge(metadata: &mut Value, from: &str, to: &str, kind: Value) {
    let path = repo_root().join("crates").join(to);
    package(metadata, from)["dependencies"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "name": to, "source": null, "req": "*", "kind": kind, "path": path,
        }));
}

#[test]
fn rejects_forbidden_edge() {
    let workspace = metadata(repo_root()).unwrap();
    check_deps(&workspace).unwrap();

    for (from, to, kind) in [
        ("ardana-api", "ardana-core", Value::Null),
        ("ardana-core", "ardana-registry", Value::Null),
        ("ardana-llama", "ardana-api", Value::Null),
        ("ardana-registry", "ardana-llama", json!("dev")),
        ("ardana-server", "ardana-llama", json!("build")),
        ("ardana-playground", "ardana-registry", Value::Null),
        ("ardana-server", "ardana-candle", Value::Null),
        ("ardana-candle", "ardana-registry", Value::Null),
        ("ardana-candle", "ardana-api", json!("dev")),
    ] {
        let mut broken = workspace.clone();
        add_edge(&mut broken, from, to, kind);
        let err = check_deps(&broken).unwrap_err().to_string();
        assert!(
            err.contains(&format!("forbidden dependency edge {from} -> {to}")),
            "{err}"
        );
    }

    for (from, to) in [
        ("ardana-core", "ardana-api"),
        ("ardana-server", "ardana-registry"),
        ("ardana-candle", "ardana-core"),
        ("ardana", "ardana-candle"),
    ] {
        let mut allowed = workspace.clone();
        add_edge(&mut allowed, from, to, Value::Null);
        check_deps(&allowed).unwrap();
    }

    let mut missing = workspace.clone();
    missing["packages"]
        .as_array_mut()
        .unwrap()
        .retain(|p| p["name"] != "ardana-llama");
    let err = check_deps(&missing).unwrap_err().to_string();
    assert!(
        err.contains("missing workspace member ardana-llama"),
        "{err}"
    );
}

/// The page's own wasm carries no core: the playground reaches the API's types alone, and the core's tokenizer,
/// planner and readout come to the tab in the engine module, which reads with the core and nothing else of the
/// workspace below the API.
#[test]
fn only_the_engine_module_uses_core() {
    let workspace = metadata(repo_root()).unwrap();
    let uses = |name: &str| -> Vec<&str> {
        let packages = workspace["packages"].as_array().unwrap();
        let package = packages.iter().find(|p| p["name"] == name).unwrap();
        package["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|dep| dep["name"].as_str())
            .filter(|dep| dep.starts_with("ardana"))
            .collect()
    };
    assert_eq!(uses("ardana-playground"), ["ardana-api"]);
    assert_eq!(uses("ardana-engine"), ["ardana-api", "ardana-core"]);
    for (from, forbidden) in [
        (
            "ardana-playground",
            &[
                "ardana-core",
                "ardana-engine",
                "ardana-registry",
                "ardana-server",
                "ardana-llama",
                "ardana",
            ][..],
        ),
        (
            "ardana-engine",
            &[
                "ardana-playground",
                "ardana-registry",
                "ardana-server",
                "ardana-llama",
                "ardana",
            ][..],
        ),
    ] {
        for to in forbidden {
            let mut broken = workspace.clone();
            add_edge(&mut broken, from, to, Value::Null);
            let err = check_deps(&broken).unwrap_err().to_string();
            assert!(
                err.contains(&format!("forbidden dependency edge {from} -> {to}")),
                "{err}"
            );
        }
    }
}

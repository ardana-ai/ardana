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
        ("ardana-playground", "ardana-core", Value::Null),
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

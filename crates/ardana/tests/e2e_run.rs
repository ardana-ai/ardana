//! R2.7: `ardana run` answers the ticket fixture with the official decider-2b Q4_K_M GGUF, on Metal (all layers
//! offloaded) and on the CPU (`--gpu-layers 0`).

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, ensure};
use serde_json::Value;

/// A file of the pinned decider-2b-GGUF snapshot that `cargo xtask fetch` put into `$HF_HOME/hub` (`tmp/hf`).
fn decider_2b_file(name: &str) -> Result<PathBuf> {
    let hf_home = std::env::var_os("HF_HOME").context("HF_HOME is not set")?;
    let repo = PathBuf::from(hf_home).join("hub/models--Mapika--decider-2b-GGUF");
    let rev = std::fs::read_to_string(repo.join("refs/main")).with_context(|| {
        format!(
            "{} has no refs/main; run `cargo xtask fetch`",
            repo.display()
        )
    })?;
    let path = repo.join("snapshots").join(rev.trim()).join(name);
    ensure!(
        path.is_file(),
        "{} is missing; run `cargo xtask fetch`",
        path.display()
    );
    Ok(path)
}

fn ticket() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/requests/ticket.json")
}

fn ardana_run(extra: &[&str]) -> Result<Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_ardana"))
        .arg("run")
        .arg("--gguf")
        .arg(decider_2b_file("decider-2b-v11-Q4_K_M.gguf")?)
        .arg("--tokenizer")
        .arg(decider_2b_file("tokenizer.json")?)
        .arg("--config")
        .arg(decider_2b_file("decider_config.json")?)
        .arg("--request")
        .arg(ticket())
        .args(extra)
        .output()
        .context("running ardana run")?;
    ensure!(
        output.status.success(),
        "ardana run {extra:?} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout)?;
    println!("ardana run {extra:?}:\n{stdout}");
    serde_json::from_str(&stdout).context("ardana run printed JSON")
}

/// The ticket checks; returns the argmax of every answer.
fn check_ticket(resp: &Value) -> Result<(String, bool)> {
    assert_eq!(resp["model"], "decider-2b-v11");
    let department = &resp["answers"]["department"];
    assert_eq!(department["type"], "choice");
    assert_eq!(department["choice"], "billing", "{department}");
    let probabilities = department["probabilities"]
        .as_object()
        .context("probabilities")?;
    assert_eq!(
        probabilities.keys().collect::<Vec<_>>(),
        ["billing", "technical", "sales"]
    );
    let sum: f64 = probabilities.values().filter_map(Value::as_f64).sum();
    assert!((sum - 1.0).abs() <= 0.001, "probabilities sum to {sum}");
    let refund = resp["answers"]["refund"]["noul"]
        .as_f64()
        .context("refund noul")?;
    assert!(refund >= 0.9, "refund {refund}");
    assert_eq!(resp["usage"]["output_tokens"], 0);
    assert!(
        resp["usage"]["input_tokens"]
            .as_u64()
            .is_some_and(|n| n > 0)
    );
    Ok((
        department["choice"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        refund > 0.5,
    ))
}

#[test]
#[ignore = "e2e: decider-2b Q4_K_M GGUF in tmp/hf (cargo xtask fetch), Metal and CPU"]
fn decider_2b_ticket() -> Result<()> {
    let metal = check_ticket(&ardana_run(&[])?)?;
    let cpu = check_ticket(&ardana_run(&["--gpu-layers", "0"])?)?;
    assert_eq!(metal, cpu, "the CPU run has the same argmaxes");
    Ok(())
}

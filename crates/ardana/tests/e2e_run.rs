//! R2.7: `ardana run` answers the ticket fixture with the official decider-2b Q4_K_M GGUF, on Metal (all layers
//! offloaded) and on the CPU (`--gpu-layers 0`).

mod common;

use std::ffi::OsString;

use anyhow::{Context, Result};
use serde_json::Value;

const DECIDER_2B: &str = "Mapika/decider-2b-GGUF";

fn ardana_run(extra: &[&str]) -> Result<Value> {
    let mut args: Vec<OsString> = vec!["--gguf".into()];
    args.push(common::hf_file(DECIDER_2B, "decider-2b-v11-Q4_K_M.gguf")?.into());
    args.push("--tokenizer".into());
    args.push(common::hf_file(DECIDER_2B, "tokenizer.json")?.into());
    args.push("--config".into());
    args.push(common::hf_file(DECIDER_2B, "decider_config.json")?.into());
    args.push("--request".into());
    args.push(common::request("ticket.json").into());
    args.extend(extra.iter().map(OsString::from));
    common::ardana_run(args)
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
    let sum = common::probability_sum(department)?;
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

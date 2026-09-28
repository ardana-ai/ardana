//! R2.7: `ardana run` answers the ticket fixture with the official decider-2b Q4_K_M GGUF, on Metal (all layers
//! offloaded) and on the CPU (`--gpu-layers 0`).

mod common;

use std::ffi::OsString;

use anyhow::Result;
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

#[test]
#[ignore = "e2e: decider-2b Q4_K_M GGUF in tmp/hf (cargo xtask fetch), Metal and CPU"]
fn decider_2b_ticket() -> Result<()> {
    let metal = common::check_ticket(&ardana_run(&[])?)?;
    let cpu = common::check_ticket(&ardana_run(&["--gpu-layers", "0"])?)?;
    assert_eq!(metal, cpu, "the CPU run has the same argmaxes");
    Ok(())
}

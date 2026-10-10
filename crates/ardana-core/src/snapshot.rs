//! A Hugging Face model snapshot: the directory of a repository commit that holds `config.json` and the model's
//! safetensors weights, as `ardana pull` records it and a runtime loads it. The architecture comes from
//! `config.json#architectures`, never from a model name.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The model's configuration, naming its architectures.
pub const CONFIG: &str = "config.json";
/// The index of a sharded checkpoint: `weight_map` names the shard holding each tensor.
pub const INDEX: &str = "model.safetensors.index.json";
/// The weights of a checkpoint in one file.
pub const SINGLE: &str = "model.safetensors";
/// The suffix of every safetensors file.
pub const SUFFIX: &str = ".safetensors";

/// `config.json#architectures`: the model classes the checkpoint names, empty when it names none.
pub fn architectures(config: &Value) -> Vec<String> {
    config["architectures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|name| name.as_str().map(str::to_string))
        .collect()
}

/// The shard files `model.safetensors.index.json#weight_map` names, each once, sorted; an error when the index has no
/// such map or names a file that is no safetensors file at the repository root.
pub fn shards(index: &Value) -> Result<Vec<String>, String> {
    let map = index["weight_map"]
        .as_object()
        .ok_or_else(|| "it has no weight_map".to_string())?;
    let mut shards: Vec<String> = Vec::new();
    for file in map.values() {
        let file = file
            .as_str()
            .ok_or_else(|| format!("its weight_map names {file}, not a file"))?;
        let plain = !file.contains(['/', '\\']) && file != SUFFIX && file.ends_with(SUFFIX);
        if !plain {
            return Err(format!(
                "its weight_map names {file:?}, not a {SUFFIX} file at the repository root"
            ));
        }
        if !shards.iter().any(|s| s == file) {
            shards.push(file.to_string());
        }
    }
    if shards.is_empty() {
        return Err("its weight_map is empty".into());
    }
    shards.sort();
    Ok(shards)
}

/// The weight files of the snapshot directory `dir`: the shards its index names ([`shards`]), else its one
/// `model.safetensors`; an error when it holds neither or its index does not read.
pub fn weight_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let index = dir.join(INDEX);
    if index.is_file() {
        let text = std::fs::read_to_string(&index)
            .map_err(|err| format!("reading {}: {err}", index.display()))?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|err| format!("parsing {}: {err}", index.display()))?;
        let shards = shards(&value).map_err(|msg| format!("{}: {msg}", index.display()))?;
        return Ok(shards.iter().map(|shard| dir.join(shard)).collect());
    }
    let single = dir.join(SINGLE);
    if single.is_file() {
        Ok(vec![single])
    } else {
        Err(format!(
            "{} holds neither {INDEX} nor {SINGLE}",
            dir.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn architectures_of_a_config() {
        let config = json!({"architectures": ["Qwen3ForCausalLM"], "model_type": "qwen3"});
        assert_eq!(architectures(&config), ["Qwen3ForCausalLM"]);
        assert!(architectures(&json!({"model_type": "qwen3"})).is_empty());
    }

    #[test]
    fn shards_of_an_index() {
        let index = json!({"metadata": {}, "weight_map": {
            "lm_head.weight": "model-00002-of-00002.safetensors",
            "model.embed_tokens.weight": "model-00001-of-00002.safetensors",
            "model.norm.weight": "model-00002-of-00002.safetensors",
        }});
        assert_eq!(
            shards(&index).unwrap(),
            [
                "model-00001-of-00002.safetensors",
                "model-00002-of-00002.safetensors"
            ]
        );
        for bad in [
            json!({}),
            json!({"weight_map": {}}),
            json!({"weight_map": {"a": 1}}),
            json!({"weight_map": {"a": "../model.safetensors"}}),
            json!({"weight_map": {"a": "model.bin"}}),
        ] {
            assert!(shards(&bad).is_err(), "{bad}");
        }
    }
}

"""Prints, as `{"model_type", "native"}`, whether transformers implements a checkpoint's model itself.

    python -I native.py <config.json>

A `config.json` with `auto_map` names Python files the checkpoint ships. transformers runs them only under
`trust_remote_code`; without it, it loads the model by its own classes when it has them for the config's `model_type`
(`resolve_trust_remote_code`), and refuses otherwise. TRL's `create_model_from_path` loads the class the config's
`architectures` names, else `AutoModelForCausalLM`'s for the type, so `native` is whether either is transformers' own.
"""

import json
import sys

import transformers
from transformers.models.auto.configuration_auto import CONFIG_MAPPING_NAMES
from transformers.models.auto.modeling_auto import MODEL_FOR_CAUSAL_LM_MAPPING_NAMES


def main(path: str) -> None:
    with open(path, encoding="utf-8") as f:
        config = json.load(f)
    model_type = config.get("model_type")
    architectures = config.get("architectures") or []
    native = model_type in CONFIG_MAPPING_NAMES and (
        (bool(architectures) and hasattr(transformers, architectures[0]))
        or model_type in MODEL_FOR_CAUSAL_LM_MAPPING_NAMES
    )
    print(json.dumps({"model_type": model_type, "native": native}))


if __name__ == "__main__":
    main(*sys.argv[1:])

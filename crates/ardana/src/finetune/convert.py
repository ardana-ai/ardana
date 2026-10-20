"""Runs llama.cpp's converter, `convert_hf_to_gguf.py`, as `python <converter>` runs it, with one change.

    python -I convert.py <converter> <checkpoint config.json> <converter arguments...>

A NemotronH checkpoint whose own `config.json` declares no `num_experts_per_tok` converts as the dense model it is. The
converter's `ModelBase.load_hparams` reads the config through transformers' `AutoConfig`, whose 5.18 `NemotronHConfig`
fills in a mixture of experts for every NemotronH (`num_experts_per_tok` 2, `n_routed_experts` 8, `n_group`, ...) and
writes the checkpoint's `hybrid_override_pattern` as a `layers_block_type` list instead. The converter then takes the
model for a mixture of experts (`conversion/nemotron.py#NemotronHModel`, `has_moe_params`), its generic metadata names
8 experts even on the dense path, which llama.cpp's loader aborts on (`n_expert_used_max > 0`), and from the list it
finds no MLP layer, as it counts `moe` alone (`_MLP_LAYER_TYPES`), where the pattern's `-` is one. So for that model
type, when the checkpoint declares no `num_experts_per_tok`, the converter reads every setting the checkpoint declares
as it declares it, and none of those mixture-of-experts settings it does not.
"""

import json
import runpy
import sys
from pathlib import Path

# The mixture-of-experts settings transformers 5.18's `NemotronHConfig` fills in for every NemotronH.
NEMOTRON_H_MOE = [
    "num_experts_per_tok",
    "n_routed_experts",
    "n_shared_experts",
    "n_group",
    "topk_group",
    "norm_topk_prob",
    "routed_scaling_factor",
    "moe_intermediate_size",
    "moe_shared_expert_intermediate_size",
    "moe_shared_expert_overlap",
    "moe_latent_size",
]


def main(converter: str, checkpoint_config: str, *args: str) -> None:
    here = Path(converter).parent
    # As the converter puts them on its path: its `conversion/` package and the `gguf` package in `gguf-py/`.
    sys.path[:0] = [str(here), str(here / "gguf-py")]
    from conversion.base import ModelBase

    with open(checkpoint_config, encoding="utf-8") as f:
        declared = json.load(f)
    load_hparams = ModelBase.load_hparams

    def as_declared(dir_model, is_mistral_format):
        hparams = load_hparams(dir_model, is_mistral_format)
        if hparams.get("model_type") == "nemotron_h" and "num_experts_per_tok" not in declared:
            hparams.update(declared)
            for key in NEMOTRON_H_MOE:
                if key not in declared:
                    hparams.pop(key, None)
        return hparams

    ModelBase.load_hparams = staticmethod(as_declared)
    sys.argv = [converter, *args]
    runpy.run_path(converter, run_name="__main__")


if __name__ == "__main__":
    main(*sys.argv[1:])

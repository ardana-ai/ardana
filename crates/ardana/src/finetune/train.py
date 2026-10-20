"""Runs TRL's `sft` script, the trainer `ardana finetune` runs, as `python -m trl.scripts.sft` runs it, on `device`.

    python -I train.py <device> <sft arguments...>

On `mps` alone, TRL's fused LM head scores the vocabulary in tiles of 16,384 instead of 32,768. Off CUDA its backward
multiplies each tile in float32 (`_addmm_fp32`), and on Metal a float32 product summing 32,768 terms hangs the GPU at
random (`kIOGPUCommandBufferCallbackErrorHang`: about 3 calls in 100 on an M1 Max under macOS 27 and torch 2.14.1),
after which torch reads zeros for the work it lost and trains on. Of about 25,000 float32 products summing 24,576
terms or fewer none hung, nor did any bfloat16 one. The tiles fold the log-softmax online, so their width changes no
result.

On every device, the LoRA of a model whose Mamba layers PEFT will not adapt leaves their output projection and
convolution out: PEFT's own "all-linear" takes them in and then refuses them
(`tuners_utils.py#_check_lora_target_modules_mamba`), so the run would end before training.
"""

import json
import os
import sys

import trl
import trl.trainer.utils
from trl.scripts import sft

# The vocabulary tile on Metal: the longest float32 sum it was seen to run.
METAL_TILE = 16384
# The model types and modules `_check_lora_target_modules_mamba` refuses LoRA on, as PEFT 0.21.2 names them.
MAMBA_TYPES = {"falcon_h1", "mamba", "mamba2", "falcon_mamba", "nemotron_h"}
MAMBA_EXCLUDED = ["out_proj", "conv1d"]


def main(device: str, *args: str) -> None:
    if device == "mps":
        trl.trainer.utils._CHUNKED_LOGPROB_CHUNK_SIZE = METAL_TILE
    get_peft_config = trl.get_peft_config

    def without_mamba_projections(model_args):
        config = get_peft_config(model_args)
        with open(os.path.join(model_args.model_name_or_path, "config.json"), encoding="utf-8") as f:
            model_type = json.load(f).get("model_type")
        if config is not None and model_type in MAMBA_TYPES:
            config.exclude_modules = MAMBA_EXCLUDED
        return config

    # `sft.main` imports it from `trl` when it runs.
    trl.get_peft_config = without_mamba_projections
    sys.argv = [sft.__file__, *args]
    parser = sft.make_parser()
    script_args, training_args, model_args, dataset_args = parser.parse_args_and_config(fail_with_unknown_args=False)
    sft.main(script_args, training_args, model_args, dataset_args)


if __name__ == "__main__":
    main(*sys.argv[1:])

"""Merges the LoRA adapter `ardana finetune` trained into its base checkpoint and saves the merged model.

    python -I merge.py <checkpoint> <adapter> <out> <dtype> [--trust-remote-code] [--key-mapping <JSON>]

The checkpoint loads as TRL's trainer loaded it (`create_model_from_path`, in `dtype`, running the code it ships only
under `--trust-remote-code`, a multimodal checkpoint's text tower through the `key_mapping` the trainer was given) but
on the CPU whatever the device trained, as it does on the CPU and MPS: the merge needs no accelerator, and a base
trained with QLoRA is larger in `dtype` than the GPU that trained it in 4-bit. The adapter loads through PEFT, and
`merge_and_unload` folds it into the base weights: neither TRL nor PEFT has a command for this. A text tower is saved
under the text model's own tensor names, not mapped back to the multimodal checkpoint's. A generation config that
transformers' own check refuses, which `save_pretrained` then will not write (NVIDIA's Nemotron 3 Nano 4B sets `top_p`
without `do_sample`), is saved as the default one for the model's config: nothing reads it, neither Ardana nor the
converter.
"""

import json
import sys

from peft import PeftModel
from transformers import GenerationConfig
from trl.trainer.utils import create_model_from_path


def main(checkpoint: str, adapter: str, out: str, dtype: str, *flags: str) -> None:
    trust_remote_code = "--trust-remote-code" in flags
    key_mapping = json.loads(flags[flags.index("--key-mapping") + 1]) if "--key-mapping" in flags else None
    kwargs = {"key_mapping": key_mapping} if key_mapping else {}
    model = create_model_from_path(
        checkpoint, dtype=dtype, device_map=None, trust_remote_code=trust_remote_code, **kwargs
    )
    merged = PeftModel.from_pretrained(model, adapter).merge_and_unload()
    if merged.can_generate():
        try:
            merged.generation_config.validate(strict=True)
        except ValueError:
            merged.generation_config = GenerationConfig.from_model_config(merged.config)
    merged.save_pretrained(out, save_original_format=key_mapping is None)


if __name__ == "__main__":
    main(*sys.argv[1:])

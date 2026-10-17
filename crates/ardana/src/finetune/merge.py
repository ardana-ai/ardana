"""Merges the LoRA adapter `ardana finetune` trained into its base checkpoint and saves the merged model.

    python -I merge.py <checkpoint> <adapter> <out> <dtype> [--trust-remote-code]

The checkpoint loads as TRL's trainer loaded it (`create_model_from_path`, in `dtype`, running the code it ships only
under `--trust-remote-code`) but on the CPU whatever the device trained, as it does on the CPU and MPS: the merge needs
no accelerator, and a base trained with QLoRA is larger in `dtype` than the GPU that trained it in 4-bit. The adapter
loads through PEFT, and `merge_and_unload` folds it into the base weights: neither TRL nor PEFT has a command for this.
"""

import sys

from peft import PeftModel
from trl.trainer.utils import create_model_from_path


def main(checkpoint: str, adapter: str, out: str, dtype: str, *flags: str) -> None:
    trust_remote_code = "--trust-remote-code" in flags
    model = create_model_from_path(checkpoint, dtype=dtype, device_map=None, trust_remote_code=trust_remote_code)
    merged = PeftModel.from_pretrained(model, adapter).merge_and_unload()
    merged.save_pretrained(out)


if __name__ == "__main__":
    main(*sys.argv[1:])

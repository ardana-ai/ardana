"""Merges the LoRA adapter `ardana finetune` trained into its base checkpoint and saves the merged model.

    python -I merge.py <checkpoint> <adapter> <out> <dtype>

The checkpoint loads as TRL's trainer loaded it (`create_model_from_path`, in `dtype`), the adapter through PEFT, and
`merge_and_unload` folds it into the base weights: neither TRL nor PEFT has a command for this.
"""

import sys

from peft import PeftModel
from trl.trainer.utils import create_model_from_path


def main(checkpoint: str, adapter: str, out: str, dtype: str) -> None:
    model = create_model_from_path(checkpoint, dtype=dtype)
    merged = PeftModel.from_pretrained(model, adapter).merge_and_unload()
    merged.save_pretrained(out)


if __name__ == "__main__":
    main(*sys.argv[1:])

"""Prints, as one JSON object, the devices torch finds and the memory `ardana finetune` holds a run against.

    python -I devices.py

`cuda` is the first CUDA device's name and memory, null without one; `mps` the memory Metal recommends Apple's GPU
use (`recommendedMaxWorkingSetSize`, past which its work slows and the machine swaps), null without the device; and
`memory` the machine's physical memory, which the CPU trains in.
"""

import json
import os

import torch


def main() -> None:
    cuda = None
    if torch.cuda.is_available():
        device = torch.cuda.get_device_properties(0)
        cuda = {"name": device.name, "memory": device.total_memory}
    mps = torch.mps.recommended_max_memory() if torch.backends.mps.is_available() else None
    memory = os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")
    print(json.dumps({"cuda": cuda, "mps": mps, "memory": memory}))


if __name__ == "__main__":
    main()

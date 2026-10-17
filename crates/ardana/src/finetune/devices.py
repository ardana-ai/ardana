"""Prints, as one JSON object, the devices torch finds and the memory `ardana finetune` holds a run against.

    python -I devices.py

`cuda` is the first CUDA device's name and memory, null without one; `mps` whether Apple's Metal device is there; and
`memory` the machine's physical memory, which the CPU and MPS train in.
"""

import json
import os

import torch


def main() -> None:
    cuda = None
    if torch.cuda.is_available():
        device = torch.cuda.get_device_properties(0)
        cuda = {"name": device.name, "memory": device.total_memory}
    memory = os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")
    print(json.dumps({"cuda": cuda, "mps": torch.backends.mps.is_available(), "memory": memory}))


if __name__ == "__main__":
    main()

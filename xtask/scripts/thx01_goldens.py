"""Exports the thx01 package's answers to the request fixtures as the goldens of crates/ardana-core/tests/data/thx01 (R8.2).

Run by `cargo xtask export-thx01`, never by hand: it needs the tmp/py/thx01-export venv the step creates (thx01 1.0.0
and the CPU build of torch). The step passes a scratch directory, which it removes afterwards.

The script downloads the six files of doofz/THX-01 the package loads, at the pinned commit, into the scratch directory
(not the hub cache: `thx01.Agent` rewrites tokenizer/tokenizer_config.json in place where it loads from), loads the
package's `Agent` on the CPU (float32) and writes, for every fixture, `Agent.system_one(state, questions)` as the
package returns it, under the provenance of the run:

    <name>.json     the package's answer to tests/fixtures/requests/<name>.json
    LICENSE         the thx01 package's Apache-2.0 license, copied from its distribution

    python thx01_goldens.py <org/repo@commit> <requests dir> <scratch dir> <output dir>
"""
import json
import shutil
import sys
from importlib import metadata
from pathlib import Path

FILES = [
    "config.json",
    "rl_agent_config.json",
    "encoder/config.json",
    "tokenizer/tokenizer.json",
    "tokenizer/tokenizer_config.json",
    "model.safetensors",
]
REQUESTS = ["ticket", "sentiment", "score"]


def main(checkpoint, requests_dir, scratch, out_dir):
    repo, commit = checkpoint.split("@")
    requests_dir, scratch, out_dir = Path(requests_dir), Path(scratch), Path(out_dir)
    from huggingface_hub import snapshot_download
    import thx01
    import torch
    import transformers

    model_dir = snapshot_download(repo, revision=commit, allow_patterns=FILES, local_dir=str(scratch / "checkpoint"))
    agent = thx01.Agent(model_dir, device="cpu")
    assert agent.device.type == "cpu" and agent.dtype == torch.float32, (agent.device, agent.dtype)

    out_dir.mkdir(parents=True, exist_ok=True)
    for name in REQUESTS:
        request = json.loads((requests_dir / f"{name}.json").read_text())
        answer = agent.system_one(request["state"], request["questions"])
        golden = {
            "generated_by": "xtask/scripts/thx01_goldens.py (cargo xtask export-thx01)",
            "thx01": thx01.__version__,
            "checkpoint": f"{repo}@{commit}",
            "device": "cpu",
            "torch": torch.__version__,
            "transformers": transformers.__version__,
            "request": f"tests/fixtures/requests/{name}.json",
        }
        golden.update(answer)
        (out_dir / f"{name}.json").write_text(json.dumps(golden, ensure_ascii=False, indent=1) + "\n")
        print(f"{name}: {json.dumps(answer['answers'], ensure_ascii=False)} ({answer['usage']['input_tokens']} tokens)")

    licenses = [f for f in metadata.files("thx01") or [] if f.name.upper().startswith("LICENSE")]
    if not licenses:
        sys.exit("the thx01 distribution ships no LICENSE file")
    shutil.copyfile(licenses[0].locate(), out_dir / "LICENSE")
    print(f"exported {len(REQUESTS)} goldens into {out_dir}")


if __name__ == "__main__":
    main(*sys.argv[1:])

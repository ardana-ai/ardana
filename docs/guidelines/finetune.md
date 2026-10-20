# Fine-tuning guidelines

Covers `ardana finetune` (`crates/ardana/src/finetune/`): the training data and rows it builds with `ardana-core`
(`crates/ardana-core/src/finetune.rs`), the base it reads through `ardana-registry` (`crates/ardana-registry/src/base.rs`),
the training environment it builds under `$ARDANA_HOME/finetune/` (uv, uv's CPython, one venv, llama.cpp's converter),
the TRL, PEFT and PyTorch run in it, the merge, the GGUF conversion and the quantization by the linked llama.cpp
(`ardana-llama`'s `quantize`) or the safetensors checkpoint Candle runs (`--format safetensors`), the calibration
and the test report (`crates/ardana-core/src/calibrate.rs`,
`crates/ardana/src/finetune/evaluate.rs`), `ardana push`, which publishes such a model to Hugging Face
(`crates/ardana/src/push.rs`, `huggingface.md`), and the `cargo xtask e2e finetune` suite (`xtask/src/finetune.rs`).
This is the one place Ardana runs Python itself; `python-tooling.md` covers the dev-only harnesses.

## Versions
- `uv` 0.13.1 (Q3) — downloaded by `ardana` from `https://github.com/astral-sh/uv/releases/download/0.13.1/`, the
  `uv-<target>.tar.gz` of `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and `aarch64-apple-darwin`, each
  checked against the sha256 its release publishes (`.sha256` beside the archive),
  compiled into `crates/ardana/src/finetune/env.rs#UV_DIGESTS`.
- `CPython` 3.12 — uv's managed build (3.12.15 when written), the venv's interpreter.
- llama.cpp `26394b4` (`26394b4e6749a41c3633db040e0987500a5f7013`, b11074, Q25) — what `llama-cpp-sys-2` 0.1.159
  vendors: `convert_hf_to_gguf.py`, `conversion/`, `gguf-py/` and `LICENSE` from GitHub's source archive
  (`/archive/<commit>.tar.gz`, sha256 `1d0fcd22eb0fb31b1f75de2cc9e585a3338b8937043662a0c222a1e9261fed29`); the
  converter imports `gguf` from that `gguf-py/`, never an installed one.
- The venv's pins (Q29), `env.rs#PACKAGES`, their own dependencies what uv resolves for them:
  `torch` 2.14.1 (the machine's build, R5.1: PyTorch's CPU wheel `2.14.1+cpu`, a CUDA wheel `2.14.1+cu126`, `+cu130` or
  `+cu132`, or PyPI's `2.14.1`), `transformers` 5.18.0, `peft` 0.21.2, `trl` 1.16.0, `accelerate` 1.15.0,
  `datasets` 5.1.0, `safetensors` 0.8.0, `tokenizers` 0.23.2, `sentencepiece` 0.2.2, `protobuf` 7.36.2, `numpy` 2.5.3;
  and `bitsandbytes` 0.50.2 (`env.rs#BITSANDBYTES`, QLoRA's 4-bit NF4) in a CUDA venv alone.

## Rules

### Environment
- Everything the command installs lives in `$ARDANA_HOME/finetune/`: `bin/uv`, `python/` (`UV_PYTHON_INSTALL_DIR`),
  `cache/` (`UV_CACHE_DIR`), `venv/`, `llama.cpp-<commit>/`, and each run's working directory `work/<name>-<pid>/`
  (the process's id, so two runs building one name never remove each other's files). uv runs
  with `UV_MANAGED_PYTHON=1` (no system interpreter), `UV_NO_CONFIG=1` (no `uv.toml` of the user's) and
  `UV_PYTHON_BIN_DIR` in the same directory, so nothing lands in `~/.cache/uv`, `~/.local/share/uv` or `~/.local/bin`.
- Downloads go through `<file>.part`, renamed only once their sha256 is the compiled one; a digest that differs is an
  error naming both, and nothing is unpacked. Only the four parts of the llama.cpp archive are unpacked, each path
  made of plain components, and only regular files and directories: a kept link (symbolic or hard) or any other entry
  is refused (`env::tests::unpack_refuses_links`). uv's archive holds a directory and two files, llama.cpp's
  3,612 files and 375 directories, no link among them.
- The first run builds what is missing; later runs reuse it: `bin/uv` when `uv --version` says 0.13.1, the venv when
  its `pyvenv.cfg` names uv's 3.12 under `python/` and its `ardana-torch-build` names this machine's torch build, the
  converter's directory when it exists. `uv pip install` of the pins runs every time, which checks the installed
  distributions and downloads nothing when they are there.
- Nothing a run does changes the environment's files (R2.5): `uv pip install --compile-bytecode` writes every
  module's bytecode when it installs (a satisfied install, `Checked 11 packages`, compiles and touches nothing), and
  every Python process runs with `-B`, so no import writes a `__pycache__` into the venv or the converter's
  directory. Without both, each new architecture's first run would add the bytecode of the modules it imports.
- torch is the build the machine runs (`env.rs#TorchBuild`, R5.1, `TorchBuild::of(os, arch, nvidia)`, a pure function
  `finetune::tests::torch_build_follows_the_machine` checks):
  - Linux where `nvidia-smi` answers (exits 0, a GPU listed): CUDA's wheels through `--torch-backend auto`, which
    reads the driver's version and tries PyTorch's CUDA indexes the driver runs, newest first (torch 2.14.1 has
    `cu132`, `cu130` and `cu126`), then the CPU index; the CUDA venv alone also gets `bitsandbytes==0.50.2`;
  - macOS arm64: PyPI's wheel (no `--torch-backend`), which runs MPS;
  - Linux without the driver: PyTorch's CPU wheels through `--torch-backend cpu`.
  Intel macOS is refused before anything (`runs_here`): torch 2.14.1 ships no macOS x86_64 wheel, so the install
  would fail only after uv and CPython downloaded.
  `--torch-backend` routes the PyTorch packages alone to PyTorch's index; `--extra-index-url` would not do: under uv's
  first-index strategy every package that index mirrors (numpy among them) is then looked up there alone, and
  `numpy==2.5.3` is not. `UV_TORCH_BACKEND` is removed from uv's environment, so the build is ardana's choice alone.
- The venv records its build in `venv/ardana-torch-build` (`cuda`, `pypi` or `cpu`), written when it is created. A
  venv made for another build (a CPU venv on a machine whose NVIDIA driver came later, or the other way) is created
  again with `--clear`: `torch==2.14.1` matches `2.14.1+cpu` as well as `2.14.1+cu130`, so `uv pip install` alone
  would keep the wrong wheel, and a venv that stops being CUDA's must lose bitsandbytes too. uv's cache keeps the
  wheels, so going back costs no download.
- Every Python process runs the venv's interpreter with `HF_HUB_OFFLINE=1` and `TRANSFORMERS_OFFLINE=1` on the
  checkpoint's snapshot directory, which `ardana` downloaded with hf-hub; `HF_HOME` (the datasets cache among it) and
  `TMPDIR` point into the run's working directory. The trainer, `merge.py` and `convert.py` run with `-I`
  (`convert.py` puts the converter's directory, whose `conversion/` it imports, on the path itself), which ignores
  every `PYTHON*` variable, so each process runs with `-u` for unbuffered output (the trainer's losses, which TRL prints to stdout, came all at once at its end under
  `PYTHONUNBUFFERED=1`).

### Data and rows
- The JSONL line is `{"state": <string | JSON>, "questions"?: {<id>: <spec>}, "answers": {<id>: <label>}}`
  (`ardana_core::finetune`): its own questions, else `--questions` (a map, or a `/v1/systemone` body whose questions
  it gives). A label is the option's name (choice), `true`/`false` (noul), the level's name for a score, its index for
  list criteria (a JSON integer or string) and its key for a legend, or `null`. `Names::Score` carries those level
  names in level order.
- A `.csv` data file (`read_csv`, by the extension) holds the same lines: a header naming `state` and a column per
  question id of `--questions`, which a CSV always takes (refused without it, naming the flag), one record a line
  (RFC 4180: a quoted cell may hold commas, doubled quotes and line breaks; `\r\n` ends and a leading byte order mark
  read too). A record stands for `{"state": <its state cell>, "answers": {<column>: <cell>}}` and goes through the
  JSONL line's own reader (`read_fields`): a noul's `true`/`false` cell is the JSON boolean, any other cell a string
  (a list level's index reads as its name), an empty cell `null`; a question without a column is never labelled. A
  missing `state` column, a column no question has and a column named twice are refused naming it, a record whose
  cells are not the header's naming its line. The example's `line` is the file line its record starts on (the header
  is line 1), so the split and the rows are those of the JSONL lines. No CSV crate: the reader is the few lines the
  format needs.
- `read_jsonl` checks each labelled line with `Decider::plan` on the base's own tokenizer and profile and each label
  against its rendered question; an error is `<file>:<line>: question <id>: <msg>` (`DataError`). A line whose
  answers are all `null` is counted and skipped; fewer than 100 labelled lines are refused; an option with fewer
  than 50 training examples is warned about (`sparse_options`). A question is its id and its spec (the spec's JSON):
  lines asking questions of their own (decider's custom data, `q1`, `q2`, ... on every line) give one id to different
  questions, which are counted apart. A question labelled on 50 training lines or more gets a warning per sparse
  option, naming the first line labelling it; the questions labelled on fewer, every option of which is sparse, are
  counted in one line.
- The split (Q26) is by line: a SplitMix64 shuffle seeded with 0 over the labelled lines, a tenth (rounded down) to
  calibration, a tenth to test, the rest to train, each in line order.
- A `TrainingRow` is a planned row: `Decider::decodes`' ids with the gold option's label id appended, and the mask 1 on
  that token alone; an isolated level's row takes `yes` (label `B`) for the gold level and `no` (`A`) for the others.
  `Plan::row_questions` names each row's question and level; a decision model's plan has no rows.
- Everything about the data happens before the environment and the checkpoint's weights (R1.4): the order of a run is
  the name (Q27), the base's checkpoint (listing and `config.json`), tokenizer and profile, the data, the environment, the checkpoint, training.

### Base
- `ardana_registry::base::base` takes every model `ardana` runs (Q6), looked up as a library size, then a registry
  entry (exactly its name), then a reference (`refs.rs`), and finds its transformers checkpoint (Q15):
  - a library size: the size's `source` (decider), else its `tokenizer` (instruct sizes: the whole repository at a
    commit); a size with neither (decider:4b) is refused naming `--checkpoint` (Q18);
  - `hf.co/<org>/<repo>` of a safetensors checkpoint (`config.json`, no GGUF): itself, at `main`;
  - an `hf.co/` GGUF reference, and an entry whose `source` is one: the `base_model` of the repository's card
    (`README.md` front matter, read at `main`, `Card::parse`: a scalar, a flow list or a block list) when it names
    exactly one repository, listed at `main`; a card naming none or several, or no card, is refused naming
    `--checkpoint`;
  - an `ollama:` reference, a local GGUF file, an entry pulled from either: none, refused naming `--checkpoint`.
  `--checkpoint hf.co/<org>/<repo>` (at `main`) or `--checkpoint <dir>` (a directory holding `config.json` and the
  safetensors `snapshot::weight_files` names, made absolute) wins over any of them. Every refusal names both forms.
- The checkpoint is opened before any weight: refused as a decision model when its listing (or directory) holds
  `rl_agent_config.json` (Q17), refused when it has no `config.json`, whose bytes are fetched (no weight) and read
  for `auto_map`. Its `tokenizer_config.json`, when it has one, is fetched and read the same way: an `auto_map` there
  loads the tokenizer by the checkpoint's code, and llama.cpp's converter calls
  `AutoTokenizer.from_pretrained(<dir>, trust_remote_code=True)` for many architectures (`conversion/base.py`'s
  `get_vocab_base`, which most BPE models take, `chatglm.py`, `glm.py`, `hunyuan.py`, `deepseek.py`, ...), so
  without `--trust-remote-code` it is refused there, naming the flag and the checkpoint's `*.py` (`base::remote_code`,
  Q14), the hub cache holding no weight file. A `config.json` with `auto_map` names the code that loads the model,
  but transformers runs it only under `trust_remote_code`: without it, it loads the model by its own classes when it
  implements the config's `model_type` (`resolve_trust_remote_code`; measured on a Phi-3 checkpoint whose `auto_map`
  names files it does not hold: transformers' `Phi3ForCausalLM` loads, `save_pretrained` keeps `auto_map`, and the
  converter's `AutoTokenizer(trust_remote_code=True)` and `AutoConfig` load without the files). So without the flag
  `Base::model_code` keeps the code's names, and once the environment is built, before any weight, `native.py` asks
  its transformers whether it implements the `model_type` (its config class, and the class `architectures` names or
  `AutoModelForCausalLM`'s): when it does the run says so and goes on, never running that code (phi-4:mini's `phi3`,
  nemotron-3-nano:4b's `nemotron_h`); when it does not, the run is refused naming the flag and the code
  (spark-x2.5's `spark2_5`, ling-3.0's `bailing_hybrid`; under the flag ling-3.0:tiny's `modeling_bailing_moe_v3.py`
  stops importing `einops` and `fla`, flash-linear-attention's Triton kernels, which the Q29 venv does not hold and
  macOS does not run). Under the flag transformers runs the checkpoint's code
  whether or not it has its own.
- `finetune.json#checkpoint` is `{"repo", "commit"}` for a Hub checkpoint (the commit its listing read) and
  `{"path"}` for a directory; the run prints `fine-tuning <base> from the checkpoint <it>` once the base is read.
- The base's profile is the one `ardana pull` derives: a library size's from its tokenizer at its pin, else the GGUF
  repository's, `decider_config.json` from the GGUF repository when the size names no layout; an entry's as recorded
  (its files checked in place); a reference's from the `tokenizer.json` and `decider_config.json` its repository (or
  directory) holds, else the checkpoint's tokenizer, in the plain layout with a `decider_config.json`, else chat.
- The quant (Q24) is a library size's default, else the quant an `hf.co/` reference (or an entry's source) names,
  else the `QUANTS` entry the base GGUF's file name ends in (`-Q8_0.gguf`), else `q4_k_m`; a quant ardana does not
  write is refused naming `--quant`. The license is a size's own (else its family's), else the `license` of the base
  repository's card, else of the checkpoint's.
- `Base::architectures` is the checkpoint's `config.json#architectures`, read when it is opened (no weight), which
  `--format safetensors` checks before the data.
- `Base::download` fetches `config.json`, the safetensors its index names (else `model.safetensors`) and every other
  root file but weights of other formats (the card, the license, a remote-code checkpoint's `*.py`), at the listed
  commit, into the hub cache; a directory is read in place.
- The base the report reads comes right after the checkpoint (`BaseModel`): a library size as `ardana run <base>`
  pulls it (`pull_first`: the default quant, its profile, recorded under its canonical name; an entry already pulled
  used as it is); an entry as recorded; a reference resolved as `ardana pull <reference> [--tokenizer <checkpoint>]
  --layout <its layout>` resolves it (`ardana_registry::pull`, downloading the GGUF), not recorded: a reference has no
  canonical name to record it under, and `ardana run` takes names.
- A multimodal checkpoint (`config.json` names a `*ForConditionalGeneration` and holds a `text_config`: Qwen3.5's
  `Qwen3_5ForConditionalGeneration`, Gemma 3's and Gemma 4's) trains its text tower alone, the model Ardana runs
  (`mod.rs#text_model`): `work/<name>-<pid>/text/` holds the `text_config` as `config.json` (its `architectures` the
  `*ForCausalLM` of the same prefix) and links to the checkpoint's files but `config.json` and the processor's
  (`processor_config.json`, `preprocessor_config.json`, `video_preprocessor_config.json`), so TRL's `AutoProcessor`
  finds the tokenizer: given the multimodal checkpoint it builds `Qwen3VLProcessor`, whose image processor needs
  torchvision and Pillow, which the Q29 venv does not hold. The text tower's tensors keep the multimodal names,
  `model.language_model.*` (Qwen3.5, Gemma 4) or `language_model.model.*` (Gemma 3); transformers 5.18 maps
  Qwen3.5's to the text model's itself (`conversion_mapping.py`, `qwen3_5_text`: a `PrefixChange`) and Gemma's not,
  so a Gemma text tower would load none of its weights (29 of 29 tensors of a Gemma 3 text model missing, 40 of 40
  of a Gemma 4, measured on small random models) and train from random ones with only a warning. The trainer
  therefore gets `--model_init_kwargs '{"key_mapping": <TEXT_TOWER>}'` (TRL hands `model_init_kwargs` to
  `from_pretrained`) and `merge.py` `--key-mapping <JSON>`: `^model\.language_model\.` and `^language_model\.model\.`
  to `model.`, `^language_model\.lm_head\.` to `lm_head.` (a pattern that does not match changes nothing; Qwen3.5's
  loads the same with it), and `merge.py` saves the merged text tower with `save_original_format=False`, under the
  text model's own names: by default transformers 5 maps the names back to the checkpoint's on save, which llama.cpp's
  converter reads (it drops `language_model.` from every name) but Candle's `Gemma3ForCausalLM` does not. No
  conversion of these text models' (`gemma3_text`, `gemma4_text`, `qwen3_5_text`) changes a tensor, so their own
  names are the converter's too.
- A checkpoint whose `tokenizer_config.json` names a `processor_class` is read through `work/<name>-<pid>/text/` too,
  with a copy of that file without the key: every Gemma 3 and 4 checkpoint names its multimodal processor
  (`Gemma3Processor`, `Gemma4Processor`, `Gemma4UnifiedProcessor`), the text-only `unsloth/gemma-3-1b-it` among them,
  and TRL's `AutoProcessor` would build it, failing for want of `preprocessor_config.json`. The model's own directory
  keeps the base's tokenizer files as they are.

### Training
- `python -I train.py <device> <arguments>`: TRL's `trl.scripts.sft`, the script `trl sft` launches through
  `accelerate launch`, run as `python -m` runs it (its `make_parser`, `parse_args_and_config` and `main`), one process,
  so the signal and the exit status are the trainer's own. On `mps` alone it first sets TRL's vocabulary tile
  (`trl.trainer.utils._CHUNKED_LOGPROB_CHUNK_SIZE`, which the fused LM head reads when it is called) to 16,384, from
  32,768 (Device and memory). The dataset is the run's `data/` directory,
  `train.jsonl` and `validation.jsonl` (the calibration split), each line `{"input_ids", "completion_mask"}`;
  `load_dataset` takes the splits from the file names, and TRL skips its own tokenization for a dataset with
  `input_ids` and builds the labels from `completion_mask` under `--completion_only_loss True`.
- Q23: `--use_peft True --lora_r 16 --lora_alpha 32 --lora_target_modules all-linear --learning_rate 1e-4
  --num_train_epochs <--epochs, 2> --seed 0`, `--eval_strategy epoch --save_strategy epoch --load_best_model_at_end
  True --metric_for_best_model eval_loss --greater_is_better False` (the epoch with the lowest calibration loss is the
  adapter saved), `--dtype` the run's dtype (below), `--max_length` the longest row (TRL's default of 1,024 would cut
  a long state's answer token off), `--report_to none`.
- PEFT expands `all-linear` to every `nn.Linear` but the LM head, a Mamba layer's `out_proj` among them, and then
  refuses LoRA on `out_proj` and `conv1d` for the model types it lists (`tuners_utils.py#_check_lora_target_modules_mamba`:
  `falcon_h1`, `mamba`, `mamba2`, `falcon_mamba`, `nemotron_h`), so nemotron-3-nano:4b's run ended before training
  (`Module 'out_proj' is incompatible with Mamba-based models`). TRL's CLI has no flag for PEFT's `exclude_modules`;
  `train.py` wraps `trl.get_peft_config`, which `sft.main` imports from `trl` when it runs, to set `exclude_modules`
  to those two for those types. LFM2's convolution layers, whose type PEFT does not list, keep their `out_proj`
  adapted.
- `--trust_remote_code True` only under `--trust-remote-code` (TRL's `SFTConfig` field, forwarded to
  `from_pretrained` and `AutoProcessor`): transformers then imports the checkpoint's own modules from its snapshot
  directory into `HF_HOME/modules`, the run's working directory, never the venv.
- `--gradient_checkpointing False` on the CPU: TRL turns it on by default, which on Qwen3-0.6B costs 22 s a step of 8
  rows instead of 16 s, and the CPU's memory is the system's. On CUDA and MPS it is passed `True`, TRL's default.

### Device and memory
- Once the environment is built, `devices.py` (`python -I`, in the run's working directory) asks torch which devices it
  finds: the first CUDA device's name and `total_memory` (`torch.cuda.get_device_properties(0)`), when
  `torch.backends.mps.is_available()` the working set Metal recommends Apple's GPU (`torch.mps.recommended_max_memory()`),
  and the machine's physical memory (`os.sysconf`: `SC_PAGE_SIZE` ×
  `SC_PHYS_PAGES`, on Linux and macOS alike, no psutil). `--device auto` (the default) takes `cuda`, then `mps`, then
  `cpu` (Q4); `--device cuda` or `mps` on a machine whose torch does not find it exits 1, `--device cuda: torch finds no
  CUDA device on this machine; it finds cpu` (the devices found in auto's order, a GPU with its name and memory).
- Q23's dtype follows the device: `cpu` trains in float32 (`--dtype float32 --bf16 False --use_cpu True`), `cuda` and
  `mps` in bfloat16 (`--dtype bfloat16 --bf16 True`; transformers takes bf16 on MPS from macOS 14,
  `is_torch_bf16_gpu_available`). `merge.py` merges at that dtype and `--format safetensors` keeps it (the checkpoint's
  `config.json` names it); the GGUF is converted at f32 whatever it was. `finetune.json` records `device` (`cuda`,
  `mps`, `cpu`), `dtype` (`bfloat16`, `float32`) and `qlora`.
- The memory estimate (Q16, `device.rs#Shape::needs`) is made after the device is picked and before the checkpoint
  downloads, from the checkpoint's listing and `config.json` (`Base::weight_bytes`: the Hub's `paths-info` sizes of the
  safetensors its index names, else of `model.safetensors`, at the listed commit; a directory's files; offline the
  cached files) and the rows (`--max_length`, the longest, which the run's line about the data prints). With `P` the
  parameters (the bytes over the stored dtype, 2 bytes unless `config.json` says float32), `E` the embeddings
  (`vocab_size` × `hidden_size`, twice unless `tie_word_embeddings`), `L = P - E`, `h` `hidden_size`, `i`
  `intermediate_size` (else 4h), `n` `num_hidden_layers` (the `text_config`'s for a multimodal checkpoint), `w` the
  training dtype's bytes and `T` the longest row:
  - weights `P·w`, or under QLoRA `L·9/16` (NF4 with a 4-byte scale per 64) `+ E·2`;
  - the LoRA adapter, `L·2r/h` parameters (r = 16) at 16 bytes each (the weight, its gradient, Adam's two moments);
  - activations of a step, TRL's 8 rows of `T` tokens: per token `n·23·(h+i)·w` without gradient checkpointing (the
    CPU), `n·h·w + 23·(h+i)·w` with it (CUDA, MPS: each layer's input, one layer recomputed); TRL 1.16's fused LM head
    scores only the answer positions, so no `(rows, T, vocab)` logits are held;
  - 1 GB for Python, torch and a GPU's context; 4 GB on `mps` (`MPS_RUNTIME`), where the trainer's peak footprint
    (`vmmap`'s physical footprint) passed the estimate made with 1 GB by 0.9 to 2.8 GB on five bases of 0.6B to 3B
    parameters (qwen3:0.6b 3.1 against 5.6 GB, llama-3.2:1b 4.4 against 6.2, lfm2.5:1.2b 4.4 against 5.3,
    minicpm5:1b 3.8 against 6.6, smollm3:3b 9.0 against 10.7, on the commands data), the same at every size: Metal's
    buffers, which torch's allocator keeps cached.
  The 23 bounds the trainer's peak resident memory (`VmHWM`, `/proc/<pid>/status`) measured on this CPU in float32,
  beyond the weights, the adapter and the runtime: Qwen3-0.6B on 8 rows of up to 128 tokens of the commands data peaks
  at 10.5 GB (13·(h+i) a token and layer; estimated 15.1 GB), decider:0.8b on rows of 261 tokens of the custom data at
  25.0 GB (23·(h+i): Qwen3.5's linear-attention layers run transformers' pure-torch path off CUDA, without
  flash-linear-attention; estimated 25.5 GB). On CUDA it puts Qwen3.5 bf16 LoRA at 3.3 GB (0.8B), 6.5 GB (2B) and
  11.8 GB (4B) for rows of 128 tokens, against unsloth's published 3, 5 and 10 GB (its own kernels); qwen3.5:9b at
  22.4 GB in bfloat16 and 11.4 GB with QLoRA, so a 16 GB card trains it with QLoRA. The suite's rows on this machine's
  67.3 GB: qwen3:0.6b 15.8 GB, qwen3.5:0.8b 16.0 GB, spark-x2.5:1.7b 35.3 GB, decider:0.8b 31.6 GB (rows of 136, 138,
  151 and 336 tokens).
- Metal hangs on a float32 matrix product that sums 32,768 terms, at random: about 3 calls in 100 of `[M, 32768] @
  [32768, 1024]` for M from 189 to 1,199 on the M1 Max under macOS 27.0.1 and torch 2.14.1 (93 of the 1,199 row counts,
  three calls each, the first at 189 rows), and none of about 25,000 float32 products summing 24,576 terms or fewer
  (16,384 at hidden sizes 1,024 to 5,120 and up to 4,100 rows; 8,192; 4,096), nor a bfloat16 product of 32,768 or
  151,936 terms. TRL 1.16's fused LM head makes such a product: off CUDA, where Triton does not run, it scores every
  position in vocabulary tiles of 32,768 and its backward multiplies each tile by the LM head in float32
  (`kernels/chunked_logprob.py#_addmm_fp32`). torch raises nothing when the command buffer fails: it prints `Error:
  command buffer exited with error status.` and `Caused GPU Hang Error (00000003:kIOGPUCommandBufferCallbackErrorHang)`,
  macOS then ignores the process's later submissions (`kIOGPUCommandBufferCallbackErrorSubmissionsIgnored`), every
  result reads zeros, and the trainer logs a loss and a gradient norm of 0 to the end and saves an adapter that merges
  into NaN weights (qwen3:0.6b on the commands data: a failure within the first 13 steps of 400 in each of three runs; the
  quantizer then refuses `found nan value`, a `--format safetensors` checkpoint would have been kept). Neither the
  dtype, gradient checkpointing, eager attention, the optimizer, gradient clipping, pinned memory nor autocast changes
  it; the model's own loss over full logits does not hang, nor does TRL's head in a loop of 40 steps of our own. So
  `train.py` sets the tile to 16,384 on `mps` (the head then makes ten products a step instead of five, the same
  result: the tiles fold the log-softmax online). trl 1.16.0 and torch 2.14.1 are the latest releases.
- Every step `env::run` runs (uv, the trainer, `merge.py`, the converter; `devices.py` and `native.py` do no GPU work
  and are read whole) passes its stderr on and is stopped as soon as it prints Metal's `command buffer
  exited with error status`: the run fails `the trainer (TRL sft) stopped: Metal failed a command buffer of its GPU work
  (<Metal's reason line>), …; run again, or pass --device cpu`, removing its files as any failed run does, never
  registering a model trained on lost work.
- The estimate is held against the GPU's memory on `cuda`, the working set Metal recommends Apple's GPU on `mps`
  (`torch.mps.recommended_max_memory()`, `recommendedMaxWorkingSetSize`: 55.7 GB of the M1 Max's 68.7 GB; past it
  the GPU's work slows and the machine swaps, and torch's MPS allocator sets its own limits from it), and the
  machine's memory on `cpu`. qwen3.8:27b on `mps` needs about 64 GB, under the machine's memory and over the working
  set, so it is refused before its 54 GB download, not trained in swap. When it fits, the run says `<base> trains on
  <device> in <dtype>, needing about <estimate> of the <memory> …`. On `cuda`, a run that does
  not fit in bfloat16 is estimated again with its weights in 4-bit; when that fits it trains with QLoRA, saying so
  (`… training with QLoRA, its weights in 4-bit NF4, needing about …`): TRL's `--load_in_4bit True
  --bnb_4bit_quant_type nf4` (`ModelConfig`, turned into a `BitsAndBytesConfig` by `get_quantization_config`, its
  compute dtype `--dtype`), the model placed on the GPU by `create_model_from_path`'s `device_map="auto"`, the adapter
  cast to bfloat16 by `SFTTrainer`. Anything else that does not fit exits 1 before the checkpoint downloads, naming the
  estimate and the memory: `qwen3.8:27b needs about 261.1 GB of memory to train on cpu in float32, more than the
  67.3 GB this machine has; pick a smaller base` (on `cuda` both estimates and the GPU's memory).

### Merge, GGUF and quantization
- `merge.py <checkpoint> <adapter> <out> <dtype> [--trust-remote-code] [--key-mapping <JSON>]` loads the base with
  TRL's `create_model_from_path` (the class and dtype the trainer used, `trust_remote_code` only under the flag, a text
  tower's `key_mapping`, saved then under the text model's own names: Base) with
  `device_map=None`, on the CPU whatever the device trained (the function's own choice on the CPU and MPS; on CUDA
  `"auto"` would offload the bfloat16 base of a QLoRA run, larger than the GPU), the adapter with
  `PeftModel.from_pretrained`, and saves `merge_and_unload()` with `save_pretrained`: neither TRL nor PEFT has a merge
  command. A QLoRA adapter merges into the base at bfloat16, not 4-bit. transformers 5.18's `save_pretrained` validates
  the generation config strictly and refuses to save one the checkpoint itself ships when it fails
  (nemotron-3-nano:4b's `top_p` 0.95 without `do_sample`: `GenerationConfig is invalid`), so such a config is replaced
  by `GenerationConfig.from_model_config` first; nothing reads it (the converter reads no `generation_config.json`,
  and a checkpoint's model leaves it out). The checkpoint's other root files (the
  tokenizer's) are copied beside it for the converter, its `*.py` only under `--trust-remote-code`: the converter loads
  tokenizers with `trust_remote_code=True`, so a Python file beside them would run.
- `convert.py <converter> <the trained checkpoint's config.json> <merged> --outtype f32 --model-name <name> --outfile
  <name>-float32.gguf` runs `convert_hf_to_gguf.py` as `python <it>` runs it (`runpy`, `conversion/` and `gguf-py/`
  on the path as it puts them) with one change: a NemotronH checkpoint whose own `config.json` sets no
  `num_experts_per_tok` converts as the dense model it is. The converter's `ModelBase.load_hparams` reads the config
  through transformers' `AutoConfig`, whose 5.18 `NemotronHConfig` fills in a mixture of experts for every NemotronH
  (`num_experts_per_tok` 2, `n_routed_experts` 8, `n_group` 1, `topk_group` 1, the MoE sizes, ...) and writes the
  checkpoint's `hybrid_override_pattern` (`M-M-M-MM-…`) as a `layers_block_type` list (`linear_attention`, `mlp`,
  ...). nemotron-3-nano:4b, fine-tuned, went three ways wrong: the converter took it for a mixture of experts when
  `num_experts_per_tok` was there (`conversion/nemotron.py#NemotronHModel`, `has_moe_params`; llama.cpp's `master`
  the same) and llama.cpp did not load the GGUF; without that key its generic metadata (`base.py`,
  `find_hparam(["num_local_experts", "num_experts", "n_routed_experts"])`) still named 8 experts with none used, and
  llama.cpp's loader aborted the process in the quantizer (`GGML_ASSERT(n_expert_used_max > 0)`); and from the list
  the converter found no MLP layer, as it counts `moe` alone there (`_MLP_LAYER_TYPES`) where the pattern's `-` is
  one, so llama.cpp did not load that GGUF either. `convert.py` wraps `load_hparams` so that, for `nemotron_h` with no
  `num_experts_per_tok` declared, the converter reads every setting the checkpoint declares as it declares it, the
  pattern among them, and none of the MoE settings (`NEMOTRON_H_MOE`) it does not; NVIDIA's own checkpoint, so
  converted, loads and answers in llama.cpp. The converter itself is run
  on the merged model at the training
  dtype (it has no remote-code flag: it reads `config.json` itself when `AutoConfig` will not, and its own
  architecture classes, `conversion/spark2_5.py` among them, convert the merged weights); `--no-nextn` only when `config.json` (or its `text_config`) declares `mtp_num_hidden_layers` or
  `num_nextn_predict_layers` above 0: the converter refuses the flag for an architecture without an MTP head. A
  multimodal checkpoint's merged text tower reaches it named as the checkpoint is (`mod.rs#name_for_converter`: its
  `config.json`'s `architectures` the checkpoint's `*ForConditionalGeneration`). The converter picks its class by the
  first of `architectures`, and it registers its text model under every multimodal name it converts but not always
  under the `*ForCausalLM`: b11074 has `Gemma4UnifiedForConditionalGeneration` and no `Gemma4UnifiedForCausalLM`, so
  gemma-4:12b, trained, ended with `Model Gemma4UnifiedForCausalLM is not supported`. Where it has both names
  (`Qwen3_5…`, `Gemma3…`, `Gemma4…`) they are one class, which reads no name; the tensors keep the text model's names.
- `--format safetensors` (Q9, Q10) skips the converter and the quantizer: the model is the merged checkpoint, the
  safetensors `merge.py` saved (with `model.safetensors.index.json` when sharded) moved into `out/` beside the
  `config.json` of the checkpoint as trained (the text tower's for a multimodal one), its `dtype` or `torch_dtype`
  set to the training dtype (`float32` on the CPU). `save_pretrained`'s own `config.json` is not kept: transformers 5
  writes `rope_parameters` (`{"rope_theta", "rope_type"}`) and no `rope_theta`, which candle-transformers 0.11.0's
  `qwen3::Config` requires (`missing field rope_theta`), and merging changes no setting. Its `generation_config.json`
  is left out with it.
  The format takes exactly the names of `ardana_candle::ARCHITECTURES` but `thx01::ARCHITECTURE`
  (`safetensors_architectures`, computed from the list, so a name Candle adds is taken with no other change): one of
  the checkpoint's `architectures` must be among them, as Candle picks the first it implements, else the run is
  refused once the base is read (before the data, the environment and any weight), naming the architectures, the
  accepted ones and `--format gguf`. `--quant` with it is refused before the base is read: there is no GGUF to
  quantize, and the checkpoint keeps the training dtype. `finetune.json` records `"format": "safetensors"` and
  `"quant": null`.
- `ardana_llama::quantize` calls `llama_model_quantize` from `llama-cpp-sys-2` (llama-cpp-2 wraps no quantization)
  with llama.cpp's default parameters and the file type `--quant` names, by default the base's own GGUF quant (Q24).
  `QUANTS` lists, lowercase, every type `llama-quantize` writes without an importance matrix, and `ALIASES` its three
  aliases (`q3_k`, `q4_k`, `q5_k` for the `_m` types), which the library uses: phi-4:14b's default quant is `q4_k`.

### Calibration and report
- Q11: the temperatures are fitted through the model itself, before it moves: the quantized GGUF in
  `work/<name>-<pid>/out/` is resolved as `ardana pull <that GGUF> --name <name> --layout <the base's>` would resolve
  it (not recorded); a checkpoint is `out/` itself, its tokenizer and `decider_config.json` in it giving the profile
  (`read_profile`) and `Runtimes::for_weights` its runtime, Candle. Either is loaded as `ardana run` loads an entry
  (the default window and offload), and the run prints `calibrated <name> through <runtime>`, the id of the runtime
  that loaded it (`evaluate::calibrate` returns it). Every calibration line is planned and answered by `Decider::run`
  at temperature 1 through a recording `LoadedModel`; each labelled question is one `calibrate::Record`, the label
  logits of its row or, for an isolated score question, of each level's `no`/`yes` row.
- Q30 is decider's `calibrate.py#fit_by_type`: `temperature` fitted over every record, and per type a
  `temperature_by_type` entry when the type has at least 50 records (`MIN_ANSWERS`), else the type is left out and
  takes `temperature`; each rounded to 3 places. `fit` scores 801 temperatures spaced evenly in log from 0.05 to 20,
  then refines the best between its neighbours by 40 golden-section steps in log, keeping the grid's value when the
  refinement is no better. The NLL is that of the probabilities the response computes from the logits
  (`engine::softmax`, `systemone::combine_isolated` for isolated levels, `systemone::scaled`), before their rounding
  to four places, each clipped to 1e-12 as `metrics.py` clips it. The fitted values go into `decider_config.json`,
  which the entry's profile then reads, so `ardana show <name> [--json]` prints them.
- The report: the base (as pulled) and the model (at its fitted temperatures) answer every test line through
  `Decider::answer`, as `ardana run` answers it, and `calibrate::graded` grades each labelled answer by the
  probabilities the response reports (a noul's `[1 - noul, noul]`). `Metrics` is `metrics.py#summarize`: accuracy (a
  noul answered `yes` at P(yes) ≥ 0.5, a choice and a score by the first most probable option), the mean NLL of the gold
  option, the mean Brier score (the squared distance from the gold one-hot, summed over options) and the ECE over 15
  bins `(lo, hi]` of the answered option's probability. `calibrate::report` gives them per type present (`choice`,
  `noul`, `score`) and `overall`, rounded to four places; stdout prints that table and `finetune.json#report` holds
  `{"lines", "base": {"name", "metrics": {<type>: {"answers", "accuracy", "nll", "brier", "ece"}, "overall": ...}},
  "tuned": {...}}`, the same numbers.
- The decodes run on a blocking thread (`Interrupt::blocking`, quantization too); SIGINT raises a flag they check
  between two lines, and the run then ends as any stopped run does.

### Output, registry and stopping
- The model is built in `work/<name>-<pid>/out/` and holds `<name>-<QUANT>.gguf` (under `--format safetensors`,
  `config.json` and the safetensors), the base's `tokenizer.json` with its `tokenizer_config.json` and
  `chat_template.jinja` as they are, `decider_config.json` (`version` the name, the base's `layout` and
  `isolated_levels`, the fitted `temperature` and `temperature_by_type`, `release_date`) and `finetune.json` (the
  Contract: `rows` counts the lines of each split, `report` the test report). Once calibrated and reported it is
  renamed to `$ARDANA_HOME/models/<name>/` and registered as `ardana pull <that GGUF> --name <name> --layout <the
  base's>` would record it, so the profile is read from `decider_config.json` beside it; a checkpoint is registered
  with the directory as its weights and its source, runtime `candle`. A registration that fails removes the directory.
  `$ARDANA_HOME` is made absolute first: the Python steps run in the working directory.
- A name is refused (Q27) when an entry, a library family or a library name (any spelling) holds it, when its model
  directory exists, and when it holds whitespace, `/` or `\` or starts with `.`.
- SIGINT is listened for from the start (`Interrupt`, a tokio unix signal): it stops the current child (killed),
  download or wait, and quantization ends before the run does; the working directory is removed whenever the run
  ends (`Work`'s `Drop`), so a stopped or failed run leaves no entry and no `models/<name>/` (R1.7).
- `ardana rm <name>` of an entry whose weights are in `$ARDANA_HOME/models/<name>/` beside a `finetune.json` deletes
  that directory with the entry (Q20).
- Linux, and macOS on Apple Silicon (Q21): elsewhere the command exits 1 saying so, on Intel macOS that PyTorch
  2.14.1 ships no wheel for it. bf16 on MPS needs macOS 14 (transformers' `is_torch_bf16_gpu_available`).

### Push
- `ardana push <name> hf.co/<org>/<repo> [--public] [--dry-run]` takes only an entry `finetune::made` says `ardana
  finetune` made (its weights in `$ARDANA_HOME/models/<name>/` beside a `finetune.json`, the test `ardana rm` uses),
  GGUF and safetensors alike (Q28), and a target `Ref::parse` reads as a whole `hf.co/<org>/<repo>`; then, without
  `--dry-run`, a token (`HF_TOKEN`, else `$HF_HOME/token`). Each refusal exits 1 before anything is printed or sent.
- It uploads every file of the directory but `finetune.json`, by name at the repository's root, and `README.md`, the
  card `push::card` writes from `finetune.json`: front matter `license` (the base's, lowercased as the Hub writes its identifiers; left out when null), `base_model`
  (the checkpoint's `repo` without `hf.co/`; a `{"path"}` only when it is a hub cache snapshot,
  `models--<org>--<repo>/snapshots/<commit>`, else left out with `base_model_relation`), `base_model_relation:
  finetune`, `tags` `ardana`, `system-one` and the format; a body naming the base (a local file by its name alone, no
  path of the trainer's machine) and the checkpoint at its commit, `ardana pull hf.co/<org>/<repo>:<QUANT>` (a
  checkpoint without the quant) and `ardana run <the name pull gives> --request request.json`, the Q23 settings with
  `epochs`, `device`, `dtype` and QLoRA, the split's lines, and the report as a Markdown table in `report`'s order,
  four places as stdout prints them. A `finetune.json` without a report is refused.
- stdout: `<name> to hf.co/<org>/<repo> (private|public)`, a `FILE BYTES` table (the card last), the count and total,
  then `README.md:` and the card; `--dry-run` adds `; a dry run, nothing is sent` and stops there. The upload is
  `ardana_registry::hub::push` (private unless `--public`, Q19; an existing repository only at that visibility).

### Testing
- Plain tests: `ardana-core`'s `finetune::tests` (`training_rows_are_inference_rows` on decider:2b's plain profile
  with isolated levels and Qwen3's chat template, `data_errors_name_line_and_question`, `split_is_stable_and_disjoint`),
  its `calibrate::tests` (`fitted_temperatures_minimize_nll`: on records of every kind, each fitted value's NLL at most
  that at ±0.05 and at 1.0, the type under 50 records left out; `records_read_what_the_response_reports`: a record at
  a temperature gives the probabilities `Decider::run` reports at it, and `Metrics` follows `metrics.py`) and
  `finetune::tests::csv_matches_jsonl` (the CSV twin of 121 JSONL lines, `\n` and `\r\n`, gives the same requests,
  labels, split and rows, its lines one on), `csv_errors_name_the_column`, `ardana-registry`'s
  `base::tests::cards_name_their_base_models`, and `crates/ardana/tests/finetune.rs`
  (`refuses_bad_data_before_downloading`, `refuses_taken_names`, `unknown_checkpoint_names_the_flag`: an `ollama:`
  reference in a store of the test's own, a stand-in GGUF and decider:4b refused naming `--checkpoint` with the hub
  cache unchanged, then the first two read on to their data with `--checkpoint`; `entry_resolves_checkpoint_from_card`:
  an entry recorded from `hf.co/Qwen/Qwen3-1.7B-GGUF:Q8_0`, its GGUF a stand-in, resolves to `hf.co/Qwen/Qwen3-1.7B`
  at the cached commit through the cached card; `safetensors_refuses_other_architectures`: qwen3.5:0.8b with `--format
  safetensors` refused naming `Qwen3_5ForConditionalGeneration` and `--format gguf` before its data, then with
  `--quant` before its base, the hub cache unchanged; `push_dry_run`: a GGUF model and a safetensors one written as
  `ardana finetune` writes them, stand-in weights beside Qwen3-1.7B's tokenizer files, registered as it registers
  them, the first by `ardana pull <its GGUF>` without `--layout`, which reads chat from its `decider_config.json`;
  `--dry-run` lists every file but `finetune.json` with its bytes on disk and the card's, `private` (`public` with
  `--public`), and the card's front matter, pull and run commands, settings and report table, with `HF_TOKEN` set and
  nothing reaching a loopback stand-in of the Hub at `HF_ENDPOINT`; `push_refusals`: a pulled entry, four targets
  that are no repository and three missing tokens (unset, blank, a blank `$HF_HOME/token`), each exit 1, its last
  line the reason, nothing printed and nothing reaching the stand-in), offline on the files `cargo xtask fetch` installs, the
  `Qwen/Qwen3-1.7B-GGUF` card and `Qwen/Qwen3.5-0.8B`'s `config.json` among them; `crates/ardana`'s
  `finetune::tests::safetensors_follows_candle_architectures` (every name of `ARCHITECTURES` but THX-01's taken, alone
  and after another name; THX-01, `Qwen3_5ForConditionalGeneration` and no name refused) and
  `finetune::tests::torch_build_follows_the_machine` (R5.1: `TorchBuild::of` for Linux x86_64 and aarch64 with and
  without `nvidia-smi` answering and macOS arm64, its `--torch-backend` and its packages, bitsandbytes in the
  CUDA build's alone; Intel macOS and Windows refused by `runs_here`),
  `finetune::device::tests::pick_takes_cuda_then_mps_then_cpu` (R5.2: `--device auto` over CUDA and MPS, MPS alone
  and the CPU alone, and a device named that torch does not find refused naming those it finds),
  `finetune::device::tests::fit_holds_mps_to_metals_working_set` (a 54 GB base in bfloat16 fits the M1 Max's 68.7 GB
  and not Metal's 55.7 GB working set, so `mps` refuses it naming both and the CPU refuses it in float32; Qwen3-0.6B
  trains on either, its estimate with `MPS_RUNTIME` 3 GB above the same with `RUNTIME`),
  `finetune::tests::library_quants_are_written` (every library size's default quant, phi-4:14b's `q4_k` among them),
  `finetune::tests::merged_model_gets_python_only_when_trusted`, `finetune::tests::merge_runs_remote_code_only_when_trusted`
  (with a text tower's `--key-mapping`), `finetune::tests::text_models_read_without_processors` (a plain checkpoint as
  it is; gemma-3:1b's `processor_class` dropped from a copy; a Gemma 3 and a Qwen3.5 multimodal checkpoint read as their
  `*ForCausalLM` without processor files, with `TEXT_TOWER`), `finetune::tests::converter_reads_a_tower_under_its_checkpoints_name`
  (a merged `Gemma4UnifiedForCausalLM` renamed `Gemma4UnifiedForConditionalGeneration`, nothing else changed),
  `finetune::env::tests::metal_failures_stop_the_step`
  (Metal's failure message whole, in pieces and without a reason line stops a step that would run a minute more, naming
  the reason; other stderr passes), `finetune::env::tests::unpack_refuses_links`,
  `ardana-llama`'s `quantize::tests::quants_are_named_like_the_library` (the aliases too), `ardana-registry`'s
  `base::tests::auto_map_in_either_config_runs_remote_code` and `ardana-core`'s
  `finetune::tests::sparse_options_count_each_question_apart` (`q1` given to a noul on 60 lines, a choice on 30 and a
  question of its own on each of 100 more: 102 questions, each with its own lines and options).
- `cargo xtask e2e finetune` runs `xtask/src/e2e.rs#FINETUNE_FIXTURES` in order with the release `ardana`, online, on
  one home `tmp/e2e/finetune/home` (removed when the suite ends, also on failure) over `tmp/hf`, under the home guard.
  It writes the first 1,000 lines of decider@23579f7's `teacher_data/commands.jsonl` (`tmp/src/decider`) as
  `commands.jsonl` and their two shared questions (`risk`, a safe/caution/destructive choice, and `outside`, a noul) as
  `commands.questions.json`, the same lines as `commands.csv` (`state,risk,outside`, cells quoted as RFC 4180 has
  them), and the first 200 lines of its `teacher_data/custom_questions.jsonl` as `custom.jsonl`,
  each line with its own noul, choice and score questions (ids `q1`, `q2`, ... in its order), in `tmp/e2e/finetune/`,
  which a row's `args` name relative to it. Then it starts one `ardana serve` on the home (`Server::start_on`,
  offline), which runs until the suite ends.
- The rows, in order: `qwen3:0.6b` (commands data, `cmd-risk`, `Gain::Above`), `thx-01` (refused,
  `Gain::Unchecked`), `decider:0.8b` (the custom data, plain layout with isolated levels, `--epochs 1`,
  `Gain::Within(0.05)`: its training mixture holds the custom component, so the row checks for no loss),
  `qwen3.5:0.8b` (commands data, the multimodal checkpoint, chat layout, `Gain::Above`), `hf.co-gguf`
  (`hf.co/Qwen/Qwen3-0.6B-GGUF:Q8_0` on `commands.csv`, `cmd-risk-gguf`, `--device cpu`, so a Mac trains one row on
  its CPU, `Gain::Unchecked`: R3.2 asks that it answers),
  `spark-x2.5:1.7b` (`Spark2_5ForCausalLM`, whose `config.json` has `auto_map`, `--epochs 1`,
  `Expect::Refused("--trust-remote-code")`, `Gain::Unchecked`: R3.5 as amended, the checkpoint's own code targeting
  transformers 4.57 and not loading under the pinned 5.18), `safetensors` (`hf.co/Qwen/Qwen3-0.6B` on
  the commands data, `--format safetensors`, `cmd-risk-st`, `Gain::Above`: the base answers through Candle too),
  `gemma-3:1b` (`Gemma3ForCausalLM`, whose `tokenizer_config.json` names `Gemma3Processor`, `cmd-risk-gemma3`),
  `gemma-4:e2b` (`Gemma4ForConditionalGeneration`, its text tower through `TEXT_TOWER`, `cmd-risk-gemma4`) and
  `phi-4:mini` (`Phi3ForCausalLM`, whose `config.json` has `auto_map` for a model transformers implements, trained
  without the flag, `cmd-risk-phi4`), each on the commands data with `--epochs 1` and `Gain::Above`, and `limits`
  (`qwen3.8:27b` on the commands data with `--device cpu`, `cmd-risk-27b`, `Expect::Refused` on the memory estimate,
  `Gain::Unchecked`). A refused row
  whose args pass `--device` (R5.2, R5.3; the suite's machine has no CUDA) checks that its refusal names two sizes, the
  estimate above the memory, then runs again with `--device cuda`, which must exit 1 with `--device cuda: torch finds
  no CUDA device` and the devices found (`cpu`, or `mps, cpu`), and on a Mac of 64 GiB or less (`sysctl hw.memsize`;
  a larger Mac's working set could hold the base, and the run would download and train it) with `--device mps`, which must exit 1
  naming three sizes, the estimate above the working set Metal recommends and that below the machine's memory, no
  weight file reaching the hub cache any time. A
  row refused naming `--trust-remote-code` (whose refusal shows no weight file reached the hub cache) runs again with
  the flag (`<name>-trusted.log`), which must reach the trainer importing the checkpoint's own code (the log names
  `transformers_modules`) and, when it stops there, leave no entry and no model directory; `merge.py` getting the
  flag is the unit test `finetune::tests::merge_runs_remote_code_only_when_trusted`. A row whose base is an `hf.co/` GGUF reference, once its runs are checked, fine-tunes
  `<name>-local` from that GGUF's file in the hub cache with `--checkpoint <the snapshot directory of the checkpoint
  its finetune.json recorded>` and `--epochs 1`, run to the end and checked the same way.
- What a model is checked against (`Cli::expected`): a library size's entry; for an `hf.co/` safetensors checkpoint
  the one cached snapshot of it holding `config.json` (`{"repo", "commit"}`, its tokenizer, its card's `license`, chat
  unless it has `decider_config.json`); for an `hf.co/` GGUF reference the card
  in the hub cache (its one `base_model` the checkpoint, `{"repo", "commit"}` with the commit of the one cached
  snapshot holding `config.json`, its `license`), the quant the reference names, the checkpoint's tokenizer files
  unless the GGUF repository has its own, chat unless it has `decider_config.json`; for a local GGUF `{"path"}` of
  `--checkpoint`, the quant its file name ends in and the checkpoint card's `license`. The report's base is the
  reference as given (a reference is resolved, not recorded), the model's layout the expected one.
- An `Expect::Answers` row runs three fine-tunes: one interrupted with SIGINT once the trainer's progress bar is past
  its first step (it also installs the environment, which the row then checks: uv 0.13.1, CPython 3.12 from
  `python/`, the Q29 pins by `importlib.metadata`, the converter's three parts), one whose trainer exits 1 because
  `trl/scripts/sft.py` is moved out of the venv for it (and back afterwards: uv's check reads metadata, not files),
  and one to the end, whose stdout it keeps apart, then checks: the directory, the GGUF file type (or, for
  `--format safetensors`, runtime `candle` in `ardana show` and `--json`, the directory as the weights, `config.json`
  the checkpoint's with the dtype the run trained in, every tensor of the safetensors in it, `F32` or `BF16`, by their
  headers) and `finetune.json`, whose `device` and `dtype` are the run's: its `--device`, else `mps` on macOS and `cpu`
  elsewhere (the suite's machine has no CUDA), `float32` on `cpu` and `bfloat16` on `mps`; the venv's torch is the
  machine's build, `2.14.1` (PyPI) on macOS and `2.14.1+cpu` on Linux; that the run says `calibrated <name> through <runtime>`, llama.cpp for a GGUF, candle for a
  checkpoint; the temperatures the run's stderr says it fitted against Q30 (a type's own only over 50 answers or
  more), `decider_config.json` and `ardana show <name> --json`; the base's entry, pulled from its default quant; the
  report (the base's and the model's names, every type's metrics in range, the same answers for both, the stdout table
  giving the same numbers) and the row's `Gain` on the overall accuracy; `ardana run` answering the first data line's
  request; the server listing the model in `/v1/models` and answering that request at `/v1/systemone` with the very
  answers `ardana run --json` gave, without a restart; `ardana push <name> hf.co/ardana-e2e/<name> --dry-run`
  (`Cli::pushed`, R6.4) listing every file of the model's directory but `finetune.json` with its bytes on disk and
  `README.md` with the bytes of the card it prints, the repository private; then `ardana rm`. The first row that answers records every file
  of `finetune/` but `work/` (size and modification time); each later one finds them unchanged and no line of its
  three runs saying a download or an install (`downloading uv`, `downloading llama.cpp`, `creating the training
  environment`, uv's `Downloading` and `Installed`). An `Expect::Refused` row checks the exit code, the message and
  that no weight file reached the hub cache. Each row removes every entry it added (its model, a base it pulled), its
  model directory and every hub path it added when its checks end.
- Measured by hand on Apple Silicon (M1 Max, 64 GB, macOS 27.0.1, the release `ardana`): one epoch of the suite's
  commands data on `mps` in bfloat16 for a size of each architecture the library holds, the overall test accuracy of
  the base and of the model (the trainer's peak footprint against the estimate, with `MPS_RUNTIME`): llama-3.2:1b
  `LlamaForCausalLM` 0.555 → 0.81, minicpm5:1b `LlamaForCausalLM` 0.485 → 0.83, lfm2.5:1.2b `Lfm2ForCausalLM`
  0.555 → 0.85, smollm3:3b `SmolLM3ForCausalLM` 0.68 → 0.86, granite-4.2:3b `GraniteForCausalLM` 0.41 → 0.875,
  phi-4:mini `Phi3ForCausalLM` 0.455 → 0.825 (11.8 against 12.9 GB), gemma-3:1b 0.405 → 0.83 (5.2 against 6.7),
  gemma-3:4b's text tower 0.675 → 0.86 (12.5 against 14.0) and as `--format safetensors` through Candle 0.675 →
  0.845, gemma-4:e2b's text tower 0.63 → 0.89 (14.9 against 16.3), gemma-4:12b's
  (`Gemma4UnifiedForConditionalGeneration`) 0.75 → 0.865 (30.1 against 30.8), qwen3.5:0.8b's text tower 0.355 → 0.765 (5.1
  against 6.3), olmo-3:7b `Olmo3ForCausalLM` 0.465 → 0.86 (19.3 against 20.6), apertus:8b `ApertusForCausalLM`
  0.55 → 0.81 (22.8 against 23.2), nemotron-3-nano:4b `NemotronHForCausalLM` 0.62 → 0.88 (16.4 against 13.6: its
  Mamba layers on transformers' reference torch kernels, `mamba_ssm` being CUDA's), lfm2.5:8b-a1b
  `Lfm2MoeForCausalLM` 0.42 → 0.795 (30.0 against 23.6: the estimate
  reads a mixture of experts' `intermediate_size`, not its experts'), and decider:0.8b on the custom data 0.9618 →
  0.9618 (10.9 against 6.5: rows of 336 tokens through Qwen3.5's pure-torch linear attention, the footprint growing
  from 8.7 to 10.9 GB over the epoch as torch's allocator caches blocks of every length). Training took 8 to 80
  minutes a model (0.6B at 1.7 s a step of 8 rows, 8B at 18 s, 12B at 24 s), against 81 to 90 minutes for two epochs of
  qwen3:0.6b on the CPU. Refused before any weight, as they must be: qwen3.6:35b-a3b on `mps` (about 85 GB), and
  ling-3.0:tiny without `--trust-remote-code` (`bailing_hybrid`); with it, its code wants `einops` and `fla`.

## Sources
- https://huggingface.co/docs/trl/sft_trainer — SFTTrainer: pre-tokenized datasets, `completion_mask`, `completion_only_loss`, `max_length`
- https://huggingface.co/docs/trl/clis — the `trl sft` command and its arguments
- https://github.com/huggingface/trl/blob/v1.16.0/trl/scripts/sft.py — the script `trl sft` launches: `load_dataset`, splits, PEFT config, `save_model`
- https://huggingface.co/docs/peft/developer_guides/lora — LoRA, `target_modules="all-linear"`, `merge_and_unload`
- https://huggingface.co/docs/transformers/main_classes/trainer — `TrainingArguments`: `eval_strategy`, `load_best_model_at_end`, `use_cpu`, `gradient_checkpointing`
- https://huggingface.co/docs/datasets/loading — local JSON files and split names from file names
- https://huggingface.co/docs/huggingface_hub/package_reference/environment_variables — `HF_HUB_OFFLINE`, `HF_HOME`
- https://docs.astral.sh/uv/reference/environment/ — `UV_CACHE_DIR`, `UV_PYTHON_INSTALL_DIR`, `UV_PYTHON_BIN_DIR`, `UV_MANAGED_PYTHON`, `UV_NO_CONFIG`
- https://docs.astral.sh/uv/guides/integration/pytorch/ — `--torch-backend`, the PyTorch indexes, first-index resolution
- https://github.com/astral-sh/uv/blob/0.13.1/crates/uv-torch/src/backend.rs — `auto`: the driver's version, then every CUDA index it runs, newest first, then the CPU index
- https://github.com/huggingface/trl/blob/v1.16.0/trl/trainer/model_config.py — `load_in_4bit`, `bnb_4bit_quant_type` (`nf4` by default)
- https://github.com/huggingface/trl/blob/v1.16.0/trl/trainer/utils.py — `get_quantization_config` (compute dtype `--dtype`), `create_model_from_path` (`device_map`), `add_fused_lm_head`
- https://github.com/huggingface/transformers/blob/v5.18.0/src/transformers/utils/import_utils.py — `is_torch_bf16_gpu_available`: bf16 on MPS from macOS 14
- https://huggingface.co/docs/hub/api — `POST /api/models/{repo}/paths-info/{revision}`: the sizes of the files a memory estimate reads
- https://github.com/astral-sh/uv/releases/tag/0.13.1 — the pinned uv release and its `.sha256` files
- https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/convert_hf_to_gguf.py — the converter: `--outtype`, `--no-nextn`, `--model-name`
- https://github.com/huggingface/transformers/blob/v5.18.0/src/transformers/conversion_mapping.py — the `qwen3_5_text` prefix change that loads the text tower from a multimodal checkpoint
- https://github.com/huggingface/transformers/blob/v5.18.0/src/transformers/models/auto/processing_auto.py — how `AutoProcessor` picks a processor, the tokenizer last
- https://docs.astral.sh/uv/reference/cli/#uv-pip-install — `--compile-bytecode`
- https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/tools/quantize/quantize.cpp — the file types `llama-quantize` writes
- https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/include/llama.h — `llama_model_quantize` and its parameters
- https://docs.rs/tokio/latest/tokio/signal/unix/fn.signal.html — a SIGINT listener that replaces the default disposition

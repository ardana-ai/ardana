# Fine-tuning guidelines

Covers `ardana finetune` (`crates/ardana/src/finetune/`): the training data and rows it builds with `ardana-core`
(`crates/ardana-core/src/finetune.rs`), the base it reads through `ardana-registry` (`crates/ardana-registry/src/base.rs`),
the training environment it builds under `$ARDANA_HOME/finetune/` (uv, uv's CPython, one venv, llama.cpp's converter),
the TRL, PEFT and PyTorch run in it, the merge, the GGUF conversion and the quantization by the linked llama.cpp
(`ardana-llama`'s `quantize`), and the `cargo xtask e2e finetune` suite (`xtask/src/finetune.rs`). This is the one
place Ardana runs Python itself; `python-tooling.md` covers the dev-only harnesses.

## Versions
- `uv` 0.13.1 (Q3) — downloaded by `ardana` from `https://github.com/astral-sh/uv/releases/download/0.13.1/`, the
  `uv-<target>.tar.gz` of `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin` and
  `aarch64-apple-darwin`, each checked against the sha256 its release publishes (`.sha256` beside the archive),
  compiled into `crates/ardana/src/finetune/env.rs#UV_DIGESTS`.
- `CPython` 3.12 — uv's managed build (3.12.15 when written), the venv's interpreter.
- llama.cpp `26394b4` (`26394b4e6749a41c3633db040e0987500a5f7013`, b11074, Q25) — what `llama-cpp-sys-2` 0.1.159
  vendors: `convert_hf_to_gguf.py`, `conversion/`, `gguf-py/` and `LICENSE` from GitHub's source archive
  (`/archive/<commit>.tar.gz`, sha256 `1d0fcd22eb0fb31b1f75de2cc9e585a3338b8937043662a0c222a1e9261fed29`); the
  converter imports `gguf` from that `gguf-py/`, never an installed one.
- The venv's pins (Q29), `env.rs#PACKAGES`, their own dependencies what uv resolves for them:
  `torch` 2.14.1 (PyTorch's CPU wheel, `2.14.1+cpu`), `transformers` 5.18.0, `peft` 0.21.2, `trl` 1.16.0,
  `accelerate` 1.15.0, `datasets` 5.1.0, `safetensors` 0.8.0, `tokenizers` 0.23.2, `sentencepiece` 0.2.2,
  `protobuf` 7.36.2, `numpy` 2.5.3; `bitsandbytes` 0.50.2 joins a CUDA venv alone (W5).

## Rules

### Environment
- Everything the command installs lives in `$ARDANA_HOME/finetune/`: `bin/uv`, `python/` (`UV_PYTHON_INSTALL_DIR`),
  `cache/` (`UV_CACHE_DIR`), `venv/`, `llama.cpp-<commit>/`, and each run's working directory `work/<name>/`. uv runs
  with `UV_MANAGED_PYTHON=1` (no system interpreter), `UV_NO_CONFIG=1` (no `uv.toml` of the user's) and
  `UV_PYTHON_BIN_DIR` in the same directory, so nothing lands in `~/.cache/uv`, `~/.local/share/uv` or `~/.local/bin`.
- Downloads go through `<file>.part`, renamed only once their sha256 is the compiled one; a digest that differs is an
  error naming both, and nothing is unpacked. Only the four parts of the llama.cpp archive are unpacked, each path
  made of plain components.
- The first run builds what is missing; later runs reuse it: `bin/uv` when `uv --version` says 0.13.1, the venv when
  its `pyvenv.cfg` names uv's 3.12 under `python/`, the converter's directory when it exists. `uv pip install` of
  the pins runs every time, which checks the installed distributions and downloads nothing when they are there.
- torch comes from PyTorch's CPU index through `uv pip install --torch-backend cpu`, which routes the PyTorch
  packages alone to `https://download.pytorch.org/whl/cpu`; `--extra-index-url` would not do: under uv's first-index
  strategy every package that index mirrors (numpy among them) is then looked up there alone, and `numpy==2.5.3` is
  not.
- Every Python process runs the venv's interpreter with `HF_HUB_OFFLINE=1` and `TRANSFORMERS_OFFLINE=1` on the
  checkpoint's snapshot directory, which `ardana` downloaded with hf-hub; `HF_HOME` (the datasets cache among it) and
  `TMPDIR` point into the run's working directory. The trainer and `merge.py` run with `-I`; the converter with `-E
  -s`, since it imports `conversion/` from beside itself.

### Data and rows
- The JSONL line is `{"state": <string | JSON>, "questions"?: {<id>: <spec>}, "answers": {<id>: <label>}}`
  (`ardana_core::finetune`): its own questions, else `--questions` (a map, or a `/v1/systemone` body whose questions
  it gives). A label is the option's name (choice), `true`/`false` (noul), the level's name for a score, its index for
  list criteria (a JSON integer or string) and its key for a legend, or `null`. `Names::Score` carries those level
  names in level order.
- `read_jsonl` checks each labelled line with `Decider::plan` on the base's own tokenizer and profile and each label
  against its rendered question; an error is `<file>:<line>: question <id>: <msg>` (`DataError`). A line whose
  answers are all `null` is counted and skipped; fewer than 100 labelled lines are refused; an option with fewer
  than 50 training examples is warned about.
- The split (Q26) is by line: a SplitMix64 shuffle seeded with 0 over the labelled lines, a tenth (rounded down) to
  calibration, a tenth to test, the rest to train, each in line order.
- A `TrainingRow` is a planned row: `Decider::decodes`' ids with the gold option's label id appended, and the mask 1 on
  that token alone; an isolated level's row takes `yes` (label `B`) for the gold level and `no` (`A`) for the others.
  `Plan::row_questions` names each row's question and level; a decision model's plan has no rows.
- Everything about the data happens before the environment and the checkpoint's weights (R1.4): the order of a run is
  the name (Q27), the base's listing, tokenizer and profile, the data, the environment, the checkpoint, training.

### Base
- `ardana_registry::base::base` takes a library size: its checkpoint is the size's `source` (decider), else its
  `tokenizer` (instruct sizes: the whole repository at a commit); a size with neither is refused (decider:4b). The
  checkpoint is listed at its pinned commit and refused as a decision model when the listing holds
  `rl_agent_config.json` (Q17); an `hf.co/<org>/<repo>` reference is listed first for the same refusal.
- The base's profile is the one `ardana pull` derives: the size's tokenizer at its pin, else the GGUF repository's;
  `decider_config.json` from the GGUF repository when the size names no layout. The quant is the size's default.
- `Base::download` fetches `config.json`, the safetensors its index names (else `model.safetensors`) and every other
  root file but weights of other formats, at the pinned commit, into the hub cache.

### Training
- `python -I -m trl.scripts.sft`: the script `trl sft` launches through `accelerate launch`, run directly, one process,
  so the signal and the exit status are the trainer's own. The dataset is the run's `data/` directory,
  `train.jsonl` and `validation.jsonl` (the calibration split), each line `{"input_ids", "completion_mask"}`;
  `load_dataset` takes the splits from the file names, and TRL skips its own tokenization for a dataset with
  `input_ids` and builds the labels from `completion_mask` under `--completion_only_loss True`.
- Q23: `--use_peft True --lora_r 16 --lora_alpha 32 --lora_target_modules all-linear --learning_rate 1e-4
  --num_train_epochs <--epochs, 2> --seed 0`, `--eval_strategy epoch --save_strategy epoch --load_best_model_at_end
  True --metric_for_best_model eval_loss --greater_is_better False` (the epoch with the lowest calibration loss is the
  adapter saved), `--dtype float32 --bf16 False --use_cpu True` (TRL turns bf16 on by default), `--max_length` the
  longest row (TRL's default of 1,024 would cut a long state's answer token off), `--report_to none`.
- `--gradient_checkpointing False` on the CPU: TRL turns it on by default, which on Qwen3-0.6B costs 22 s a step of 8
  rows instead of 16 s, and the CPU's memory is the system's.

### Merge, GGUF and quantization
- `merge.py <checkpoint> <adapter> <out> <dtype>` loads the base with TRL's `create_model_from_path` (the class and
  dtype the trainer used), the adapter with `PeftModel.from_pretrained`, and saves `merge_and_unload()` with
  `save_pretrained`: neither TRL nor PEFT has a merge command. The checkpoint's other root files (the tokenizer's) are
  copied beside it for the converter.
- `convert_hf_to_gguf.py <merged> --outtype f32 --model-name <name> --outfile <name>-float32.gguf`, at the training
  dtype; `--no-nextn` only when `config.json` (or its `text_config`) declares `mtp_num_hidden_layers` or
  `num_nextn_predict_layers` above 0: the converter refuses the flag for an architecture without an MTP head.
- `ardana_llama::quantize` calls `llama_model_quantize` from `llama-cpp-sys-2` (llama-cpp-2 wraps no quantization)
  with llama.cpp's default parameters and the file type `--quant` names, by default the base's own GGUF quant (Q24).
  `QUANTS` lists, lowercase, every type `llama-quantize` writes without an importance matrix.

### Output, registry and stopping
- The model is built in `work/<name>/out/` and holds `<name>-<QUANT>.gguf`, the base's `tokenizer.json` with its
  `tokenizer_config.json` and `chat_template.jinja` as they are, `decider_config.json` (`version` the name, the base's
  `layout` and `isolated_levels`, `temperature` 1.0 until W2 fits it, `release_date`) and `finetune.json` (the
  Contract: `rows` counts the lines of each split, `report` is `null`). It is renamed to `$ARDANA_HOME/models/<name>/`
  and registered as `ardana pull <that GGUF> --name <name> --layout <the base's>` would record it, so the profile is
  read from `decider_config.json` beside it; a registration that fails removes the directory.
- A name is refused (Q27) when an entry, a library family or a library name (any spelling) holds it, when its model
  directory exists, and when it holds whitespace, `/` or `\` or starts with `.`.
- SIGINT is listened for from the start (`Interrupt`, a tokio unix signal): it stops the current child (killed),
  download or wait, and quantization ends before the run does; the working directory is removed whenever the run
  ends (`Work`'s `Drop`), so a stopped or failed run leaves no entry and no `models/<name>/` (R1.7).
- `ardana rm <name>` of an entry whose weights are in `$ARDANA_HOME/models/<name>/` beside a `finetune.json` deletes
  that directory with the entry (Q20).
- Linux and macOS only (Q21): elsewhere the command exits 1 saying so.

### Testing
- Plain tests: `ardana-core`'s `finetune::tests` (`training_rows_are_inference_rows` on decider:2b's plain profile
  with isolated levels and Qwen3's chat template, `data_errors_name_line_and_question`, `split_is_stable_and_disjoint`)
  and `crates/ardana/tests/finetune.rs` (`refuses_bad_data_before_downloading`, `refuses_taken_names`), offline on the
  tokenizers `cargo xtask fetch` installs.
- `cargo xtask e2e finetune` runs `xtask/src/e2e.rs#FINETUNE_FIXTURES` in order with the release `ardana`, online, on
  one home `tmp/e2e/finetune/home` (removed when the suite ends, also on failure) over `tmp/hf`, under the home guard.
  It writes the first 1,000 lines of decider@23579f7's `teacher_data/commands.jsonl` (`tmp/src/decider`) as
  `commands.jsonl` and their two shared questions (`risk`, a safe/caution/destructive choice, and `outside`, a noul) as
  `commands.questions.json` in `tmp/e2e/finetune/`, which a row's `args` name relative to it.
- An `Expect::Answers` row runs three fine-tunes: one interrupted with SIGINT once the trainer's progress bar is past
  its first step (it also installs the environment, which the row then checks: uv 0.13.1, CPython 3.12 from
  `python/`, the Q29 pins by `importlib.metadata`, the converter's three parts), one whose trainer exits 1 because
  `trl/scripts/sft.py` is moved out of the venv for it (and back afterwards: uv's check reads metadata, not files),
  and one to the end, whose directory, GGUF file type, `finetune.json` and `ardana run` answers it checks before
  `ardana rm`. An `Expect::Refused` row checks the exit code, the message and that no weight file reached the hub
  cache. Each row removes its entry, its model directory and every hub path it added when its checks end.

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
- https://github.com/astral-sh/uv/releases/tag/0.13.1 — the pinned uv release and its `.sha256` files
- https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/convert_hf_to_gguf.py — the converter: `--outtype`, `--no-nextn`, `--model-name`
- https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/tools/quantize/quantize.cpp — the file types `llama-quantize` writes
- https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/include/llama.h — `llama_model_quantize` and its parameters
- https://docs.rs/tokio/latest/tokio/signal/unix/fn.signal.html — a SIGINT listener that replaces the default disposition

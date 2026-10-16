# Fine-tuning guidelines

Covers `ardana finetune` (`crates/ardana/src/finetune/`): the training data and rows it builds with `ardana-core`
(`crates/ardana-core/src/finetune.rs`), the base it reads through `ardana-registry` (`crates/ardana-registry/src/base.rs`),
the training environment it builds under `$ARDANA_HOME/finetune/` (uv, uv's CPython, one venv, llama.cpp's converter),
the TRL, PEFT and PyTorch run in it, the merge, the GGUF conversion and the quantization by the linked llama.cpp
(`ardana-llama`'s `quantize`), the calibration and the test report (`crates/ardana-core/src/calibrate.rs`,
`crates/ardana/src/finetune/evaluate.rs`), and the `cargo xtask e2e finetune` suite (`xtask/src/finetune.rs`). This is
the one place Ardana runs Python itself; `python-tooling.md` covers the dev-only harnesses.

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
- Nothing a run does changes the environment's files (R2.5): `uv pip install --compile-bytecode` writes every
  module's bytecode when it installs (a satisfied install, `Checked 11 packages`, compiles and touches nothing), and
  every Python process runs with `-B`, so no import writes a `__pycache__` into the venv or the converter's
  directory. Without both, each new architecture's first run would add the bytecode of the modules it imports.
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
  than 50 training examples is warned about.
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
  for `auto_map`: a checkpoint that has it loads by running the Python files it ships, so without
  `--trust-remote-code` it is refused naming the flag and those files (Q14), the hub cache holding no weight file.
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
- `Base::download` fetches `config.json`, the safetensors its index names (else `model.safetensors`) and every other
  root file but weights of other formats (the card, the license, a remote-code checkpoint's `*.py`), at the listed
  commit, into the hub cache; a directory is read in place.
- The base the report reads comes right after the checkpoint (`BaseModel`): a library size as `ardana run <base>`
  pulls it (`pull_first`: the default quant, its profile, recorded under its canonical name; an entry already pulled
  used as it is); an entry as recorded; a reference resolved as `ardana pull <reference> [--tokenizer <checkpoint>]
  --layout <its layout>` resolves it (`ardana_registry::pull`, downloading the GGUF), not recorded: a reference has no
  canonical name to record it under, and `ardana run` takes names.
- A multimodal checkpoint (`config.json` names a `*ForConditionalGeneration` and holds a `text_config`, Qwen3.5's
  `Qwen3_5ForConditionalGeneration`) trains its text tower alone, the model Ardana runs: `work/<name>/text/` holds the
  `text_config` as `config.json` (its `architectures` the `*ForCausalLM` of the same prefix) and links to the
  checkpoint's files but `config.json` and the processor's (`processor_config.json`, `preprocessor_config.json`,
  `video_preprocessor_config.json`). transformers loads `Qwen3_5ForCausalLM` from the checkpoint's
  `model.language_model.*` tensors (`conversion_mapping.py`, `qwen3_5_text`: a `PrefixChange`), and TRL's
  `AutoProcessor` then finds the tokenizer: given the multimodal checkpoint it builds `Qwen3VLProcessor`, whose image
  processor needs torchvision and Pillow, which the Q29 venv does not hold. The trainer, `merge.py` and the converter
  all read the text tower, so the GGUF is the text model.

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
- `--trust_remote_code True` only under `--trust-remote-code` (TRL's `SFTConfig` field, forwarded to
  `from_pretrained` and `AutoProcessor`): transformers then imports the checkpoint's own modules from its snapshot
  directory into `HF_HOME/modules`, the run's working directory, never the venv.
- `--gradient_checkpointing False` on the CPU: TRL turns it on by default, which on Qwen3-0.6B costs 22 s a step of 8
  rows instead of 16 s, and the CPU's memory is the system's.

### Merge, GGUF and quantization
- `merge.py <checkpoint> <adapter> <out> <dtype> [--trust-remote-code]` loads the base with TRL's
  `create_model_from_path` (the class and dtype the trainer used, `trust_remote_code` only under the flag), the adapter with `PeftModel.from_pretrained`, and saves `merge_and_unload()` with
  `save_pretrained`: neither TRL nor PEFT has a merge command. The checkpoint's other root files (the tokenizer's) are
  copied beside it for the converter.
- `convert_hf_to_gguf.py <merged> --outtype f32 --model-name <name> --outfile <name>-float32.gguf`, at the training
  dtype (it has no remote-code flag: it reads `config.json` itself when `AutoConfig` will not, and its own
  architecture classes, `conversion/spark2_5.py` among them, convert the merged weights); `--no-nextn` only when `config.json` (or its `text_config`) declares `mtp_num_hidden_layers` or
  `num_nextn_predict_layers` above 0: the converter refuses the flag for an architecture without an MTP head.
- `ardana_llama::quantize` calls `llama_model_quantize` from `llama-cpp-sys-2` (llama-cpp-2 wraps no quantization)
  with llama.cpp's default parameters and the file type `--quant` names, by default the base's own GGUF quant (Q24).
  `QUANTS` lists, lowercase, every type `llama-quantize` writes without an importance matrix.

### Calibration and report
- Q11: the temperatures are fitted through the quantized GGUF itself, before it moves: `work/<name>/out/` is resolved
  as `ardana pull <that GGUF> --name <name> --layout <the base's>` would resolve it (not recorded) and loaded as
  `ardana run` loads an entry (the default window and offload). Every calibration line is planned and answered by
  `Decider::run` at temperature 1 through a recording `LoadedModel`; each labelled question is one `calibrate::Record`,
  the label logits of its row or, for an isolated score question, of each level's `no`/`yes` row.
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
- The model is built in `work/<name>/out/` and holds `<name>-<QUANT>.gguf`, the base's `tokenizer.json` with its
  `tokenizer_config.json` and `chat_template.jinja` as they are, `decider_config.json` (`version` the name, the base's
  `layout` and `isolated_levels`, the fitted `temperature` and `temperature_by_type`, `release_date`) and
  `finetune.json` (the Contract: `rows` counts the lines of each split, `report` the test report). Once calibrated and
  reported it is renamed to `$ARDANA_HOME/models/<name>/` and registered as `ardana pull <that GGUF> --name <name>
  --layout <the base's>` would record it, so the profile is read from `decider_config.json` beside it; a registration
  that fails removes the directory. `$ARDANA_HOME` is made absolute first: the Python steps run in the working
  directory.
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
  at the cached commit through the cached card), offline on the files `cargo xtask fetch` installs, the
  `Qwen/Qwen3-1.7B-GGUF` card among them.
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
  `Gain::Within(0.05)`: its training mixture holds the custom component, so the row checks for no loss) and
  `qwen3.5:0.8b` (commands data, the multimodal checkpoint, chat layout, `Gain::Above`), `hf.co-gguf`
  (`hf.co/Qwen/Qwen3-0.6B-GGUF:Q8_0` on `commands.csv`, `cmd-risk-gguf`, `Gain::Unchecked`: R3.2 asks that it answers)
  and `spark-x2.5:1.7b` (`Spark2_5ForCausalLM`, whose `config.json` has `auto_map`, `--trust-remote-code --epochs 1`,
  `Gain::Unchecked`). A row whose args hold `--trust-remote-code` first runs without it, which must exit 1 naming the
  flag with no weight file reaching the hub cache. A row whose base is an `hf.co/` GGUF reference, once its runs are
  checked, fine-tunes `<name>-local` from that GGUF's file in the hub cache with `--checkpoint <the snapshot
  directory of the checkpoint its finetune.json recorded>` and `--epochs 1`, run to the end and checked the same way.
- What a model is checked against (`Cli::expected`): a library size's entry; for an `hf.co/` GGUF reference the card
  in the hub cache (its one `base_model` the checkpoint, `{"repo", "commit"}` with the commit of the one cached
  snapshot holding `config.json`, its `license`), the quant the reference names, the checkpoint's tokenizer files
  unless the GGUF repository has its own, chat unless it has `decider_config.json`; for a local GGUF `{"path"}` of
  `--checkpoint`, the quant its file name ends in and the checkpoint card's `license`. The report's base is the
  reference as given (a reference is resolved, not recorded), the model's layout the expected one.
- An `Expect::Answers` row runs three fine-tunes: one interrupted with SIGINT once the trainer's progress bar is past
  its first step (it also installs the environment, which the row then checks: uv 0.13.1, CPython 3.12 from
  `python/`, the Q29 pins by `importlib.metadata`, the converter's three parts), one whose trainer exits 1 because
  `trl/scripts/sft.py` is moved out of the venv for it (and back afterwards: uv's check reads metadata, not files),
  and one to the end, whose stdout it keeps apart, then checks: the directory, the GGUF file type and
  `finetune.json`; the temperatures the run's stderr says it fitted against Q30 (a type's own only over 50 answers or
  more), `decider_config.json` and `ardana show <name> --json`; the base's entry, pulled from its default quant; the
  report (the base's and the model's names, every type's metrics in range, the same answers for both, the stdout table
  giving the same numbers) and the row's `Gain` on the overall accuracy; `ardana run` answering the first data line's
  request; the server listing the model in `/v1/models` and answering that request at `/v1/systemone` with the very
  answers `ardana run --json` gave, without a restart; then `ardana rm`. The first row that answers records every file
  of `finetune/` but `work/` (size and modification time); each later one finds them unchanged and no line of its
  three runs saying a download or an install (`downloading uv`, `downloading llama.cpp`, `creating the training
  environment`, uv's `Downloading` and `Installed`). An `Expect::Refused` row checks the exit code, the message and
  that no weight file reached the hub cache. Each row removes every entry it added (its model, a base it pulled), its
  model directory and every hub path it added when its checks end.

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
- https://github.com/huggingface/transformers/blob/v5.18.0/src/transformers/conversion_mapping.py — the `qwen3_5_text` prefix change that loads the text tower from a multimodal checkpoint
- https://github.com/huggingface/transformers/blob/v5.18.0/src/transformers/models/auto/processing_auto.py — how `AutoProcessor` picks a processor, the tokenizer last
- https://docs.astral.sh/uv/reference/cli/#uv-pip-install — `--compile-bytecode`
- https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/tools/quantize/quantize.cpp — the file types `llama-quantize` writes
- https://github.com/ggml-org/llama.cpp/blob/26394b4e6749a41c3633db040e0987500a5f7013/include/llama.h — `llama_model_quantize` and its parameters
- https://docs.rs/tokio/latest/tokio/signal/unix/fn.signal.html — a SIGINT listener that replaces the default disposition

//! The examples and notes under each command's `--help`. They name no library model but the family `decider`, the
//! default one, write any other as `<family>:<size>-<quant>` and point to the library's page instead (Q12, Q15), and
//! read nothing when clap builds the command.

use ardana_registry::library::{DEFAULT_MODEL, LIBRARY_OFF, LIBRARY_URL, LIBRARY_VAR, MODELS_PAGE};
use ardana_server::LIBRARY_REFRESH_VAR;

use crate::update::NO_CHECK_VAR;

pub fn main() -> String {
    format!(
        "Examples:
  ardana run {DEFAULT_MODEL} \"My card was charged twice.\" --noul \"Does the customer ask for a refund?\"
  ardana pull <family>:<size>-<quant>
  ardana serve

Models: the library at {MODELS_PAGE} ({DEFAULT_MODEL} is the default), or any GGUF on Hugging Face
(hf.co/<org>/<repo>).

Environment:
  ARDANA_HOME             Where the list of pulled models and the library cache live (default ~/.ardana)
  {LIBRARY_VAR}          Where the library is read from (default {LIBRARY_URL}; a file path, or off)
  {NO_CHECK_VAR}  Set to 1 to stop run and serve from looking for a newer release (once a day)
  HF_HOME                 The Hugging Face cache weights download to (default ~/.cache/huggingface)
  HF_HUB_OFFLINE          Set to 1 to use only models already in that cache
  OLLAMA_MODELS           The Ollama store ollama: models are read from (default ~/.ollama/models)"
    )
}

pub fn run() -> String {
    format!(
        "Examples:
  Ask a yes/no question:
    ardana run {DEFAULT_MODEL} \"I was charged twice for order A-104.\" --noul \"Does the customer ask for a refund?\"

  Pick one of several options, and a level on a scale (lowest first):
    ardana run {DEFAULT_MODEL} \"The app crashes on login. Fix it now!\" \\
      --choice \"Which team should handle this?\" billing technical sales \\
      --score \"How upset is the customer?\" calm annoyed furious

  Read the state from a file or a pipe:
    cat ticket.txt | ardana run {DEFAULT_MODEL} --noul \"Is an order number given?\"

  Send a whole /v1/systemone request and print the API's JSON response:
    ardana run {DEFAULT_MODEL} --request request.json --json

The state comes before the questions. A library model ({MODELS_PAGE}) that is not pulled yet is downloaded first."
    )
}

pub fn pull() -> String {
    format!(
        "Examples:
  A library model, or one size of it in another quantization:
    ardana pull {DEFAULT_MODEL}
    ardana pull <family>:<size>-<quant>

  Any GGUF repository on Hugging Face (add the tokenizer's repository when the GGUF one has none):
    ardana pull hf.co/unsloth/Qwen3-4B-GGUF
    ardana pull hf.co/unsloth/Qwen3-4B-GGUF --tokenizer hf.co/Qwen/Qwen3-4B

  A model already in your Ollama store, or a local file:
    ardana pull ollama:llama3.2
    ardana pull ./model.gguf --tokenizer ./tokenizer.json --name my-model

Library: {MODELS_PAGE}; a library family's manifest is read from {LIBRARY_VAR} (default {LIBRARY_URL}).
Weights download to the Hugging Face cache; Ollama and local files are read where they are."
    )
}

pub fn list() -> String {
    format!(
        "Library models ({MODELS_PAGE}) that are not pulled yet are downloaded by `ardana pull` or on their first run."
    )
}

pub fn show() -> String {
    format!(
        "Examples:
  ardana show {DEFAULT_MODEL}
  ardana show {DEFAULT_MODEL} --json
  ardana show <family>:<size>-<quant>    A library model not pulled yet, from the library cache"
    )
}

pub fn ps() -> String {
    "Asks the running `ardana serve` which models it has loaded, most recently used first."
        .to_string()
}

pub fn rm() -> String {
    "Examples:
  ardana rm <name>
  ardana rm <name> my-model

A name is an entry's as `ardana list` prints it. A model `ardana finetune` made is removed with its directory,
$ARDANA_HOME/models/<name>/; of any other only the entry goes: the weights stay in the Hugging Face cache, the Ollama
store or where they were."
        .to_string()
}

pub fn finetune() -> String {
    use crate::finetune::{
        DEFAULT_EPOCHS, LEARNING_RATE, LLAMA_CPP_COMMIT, LORA_ALPHA, LORA_RANK, PYTHON, SEED,
        UV_VERSION,
    };
    use ardana_core::calibrate::{ECE_BINS, MIN_ANSWERS};
    use ardana_core::finetune::{MIN_LINES, MIN_OPTION_EXAMPLES};
    let llama_cpp = &LLAMA_CPP_COMMIT[..7];
    format!(
        "Examples:
  ardana finetune <family>:<size> data.jsonl --questions questions.json --name my-model
  ardana finetune <family>:<size> data.jsonl --name my-model --epochs 1 --quant q4_k_m
  ardana finetune hf.co/<org>/<repo>-GGUF:Q8_0 data.csv --questions questions.json --name my-model
  ardana finetune model.gguf data.jsonl --checkpoint hf.co/<org>/<repo> --name my-model
  ardana run my-model --request request.json

Base: any model ardana runs: a library size, a pulled model's name, an hf.co/ reference (a safetensors checkpoint
or a GGUF), an ollama: reference or a local GGUF file. Training reads the base's transformers checkpoint: the one
the library pins for a size, an hf.co/ checkpoint itself, and for an hf.co/ GGUF (or a model pulled from one) the
base_model its repository's card names when it names exactly one. --checkpoint hf.co/<org>/<repo> or --checkpoint
<dir> names it for any base, and a base whose checkpoint is unknown is refused naming it. A checkpoint whose
config.json has auto_map loads by running the Python code it ships; it is refused unless --trust-remote-code.

Data: one JSON object a line,
  {{\"state\": <text or JSON>, \"questions\": {{<id>: <question spec>}}, \"answers\": {{<id>: <label>}}}}
the specs as a /v1/systemone request writes them, and a label the option's name (choice), true or false (noul),
the level's index or its key in a legend (score), or null where the line does not answer. A line without
\"questions\" asks those of --questions (a map of id to spec, or a /v1/systemone request). A CSV file (.csv) holds
the same lines: a header naming a state column and a column per question id of --questions, which a CSV always
takes, each cell written as a line's label, an empty cell unlabelled. The lines are split by line, seeded: 80% to
train on, 10% to calibrate on, 10% to test on. At least {MIN_LINES} labelled lines are needed; an option with fewer
than {MIN_OPTION_EXAMPLES} training examples is warned about.

Training: a LoRA of rank {LORA_RANK} and alpha {LORA_ALPHA} on every linear layer, trained with TRL's sft on the rows
ardana reads at run time, the loss on the answer token alone: learning rate {LEARNING_RATE}, {DEFAULT_EPOCHS} epochs
(--epochs), seed {SEED}, the epoch with the lowest calibration loss kept, float32 on the CPU. The merged model is
converted to GGUF and quantized to --quant, by default the quant of the base's own GGUF (the library's, the one
an hf.co/ reference or the file name names), else q4_k_m.

Calibration and report: the GGUF's temperatures are fitted on the calibration lines (one over every answer, and one
per answer type with at least {MIN_ANSWERS} answers), and `ardana show <name>` prints them. Then the base, pulled as
`ardana run <base>` pulls it, and the model answer the test lines; stdout prints the accuracy, NLL, Brier score and
{ECE_BINS}-bin ECE of both per answer type and overall, which finetune.json's report holds too.

Files: the first run downloads uv {UV_VERSION}, CPython {PYTHON}, PyTorch, Transformers, PEFT and TRL, and llama.cpp's
converter ({llama_cpp}) into $ARDANA_HOME/finetune/, which later runs reuse; the base's checkpoint goes to the
Hugging Face cache. The model is written to $ARDANA_HOME/models/<name>/ (the GGUF, the tokenizer and chat template,
decider_config.json with the fitted temperatures, and finetune.json) and registered as <name>; `ardana rm <name>`
removes both, and a run that stops leaves neither. Fine-tuning runs on Linux and macOS."
    )
}

pub fn update() -> String {
    format!(
        "Examples:
  ardana update

Installs the latest release from GitHub over this copy, in the directory the install script put it in. A copy that
came another way (a source build) has no install receipt and is told the install line instead.

`ardana run` and `ardana serve` look for a newer release once a day and say so on stderr; {NO_CHECK_VAR}=1
turns that off."
    )
}

pub fn serve() -> String {
    format!(
        "Examples:
  ardana serve                              API and playground on http://127.0.0.1:8000
  ardana serve --host 0.0.0.0 --api-key s3cret
  curl http://127.0.0.1:8000/v1/systemone -H 'content-type: application/json' -d @request.json

Endpoints: POST /v1/systemone, GET /v1/models, GET /health, and the playground at /.
A request naming a library model ({MODELS_PAGE}) that is not pulled yet pulls it first.

Library: the index at {LIBRARY_VAR} (default {LIBRARY_URL}) is read once on start and once every
--library-refresh ({LIBRARY_REFRESH_VAR}), with User-Agent ardana/<version> and nothing else about this machine;
the copy at hand serves when a read fails. {LIBRARY_VAR}={LIBRARY_OFF} sends nothing and serves the pulled models alone."
    )
}

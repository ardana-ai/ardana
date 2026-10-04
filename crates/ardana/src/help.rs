//! The examples and notes under each command's `--help`. They name no library model but the default one and point
//! to the library's page instead (Q11), and read nothing when clap builds the command.

use ardana_registry::library::{DEFAULT_MODEL, LIBRARY_OFF, LIBRARY_URL, LIBRARY_VAR, MODELS_PAGE};
use ardana_server::LIBRARY_REFRESH_VAR;

pub fn main() -> String {
    format!(
        "Examples:
  ardana run {DEFAULT_MODEL} \"My card was charged twice.\" --noul \"Does the customer ask for a refund?\"
  ardana pull {DEFAULT_MODEL}:q8_0
  ardana serve

Models: the library at {MODELS_PAGE} ({DEFAULT_MODEL} is the default), or any GGUF on Hugging Face
(hf.co/<org>/<repo>).

Environment:
  ARDANA_HOME     Where the list of pulled models and the library cache live (default ~/.ardana)
  {LIBRARY_VAR}  Where the library is read from (default {LIBRARY_URL}; a file path, or off)
  HF_HOME         The Hugging Face cache weights download to (default ~/.cache/huggingface)
  HF_HUB_OFFLINE  Set to 1 to use only models already in that cache
  OLLAMA_MODELS   The Ollama store ollama: models are read from (default ~/.ollama/models)"
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
  A library model, or another quantization of it:
    ardana pull {DEFAULT_MODEL}
    ardana pull {DEFAULT_MODEL}:q8_0

  Any GGUF repository on Hugging Face (add the tokenizer's repository when the GGUF one has none):
    ardana pull hf.co/unsloth/Qwen3-4B-GGUF
    ardana pull hf.co/unsloth/Qwen3-4B-GGUF --tokenizer hf.co/Qwen/Qwen3-4B

  A model already in your Ollama store, or a local file:
    ardana pull ollama:llama3.2
    ardana pull ./model.gguf --tokenizer ./tokenizer.json --name my-model

Library: {MODELS_PAGE}; a library model's manifest is read from {LIBRARY_VAR} (default {LIBRARY_URL}).
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
  ardana show {DEFAULT_MODEL}:q8_0       A library model not pulled yet, from the library cache"
    )
}

pub fn ps() -> String {
    "Asks the running `ardana serve` which models it has loaded, most recently used first."
        .to_string()
}

pub fn rm() -> String {
    format!(
        "Examples:
  ardana rm {DEFAULT_MODEL}
  ardana rm {DEFAULT_MODEL} my-model

Only the entries are removed: the weights stay in the Hugging Face cache, the Ollama store or where they were."
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

//! The examples and notes under each command's `--help`.

use ardana_registry::library::library;

/// The library's names, the default one marked.
fn library_names() -> String {
    let library = library();
    library
        .names()
        .iter()
        .map(|name| {
            if *name == library.default {
                format!("{name} (default)")
            } else {
                (*name).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn main() -> String {
    format!(
        "Examples:
  ardana run decider-2b \"My card was charged twice.\" --noul \"Does the customer ask for a refund?\"
  ardana pull decider-4b
  ardana serve

Models: {}, or any GGUF on Hugging Face (hf.co/<org>/<repo>).

Environment:
  ARDANA_HOME     Where the list of pulled models lives (default ~/.ardana)
  HF_HOME         The Hugging Face cache weights download to (default ~/.cache/huggingface)
  HF_HUB_OFFLINE  Set to 1 to use only models already in that cache
  OLLAMA_MODELS   The Ollama store ollama: models are read from (default ~/.ollama/models)",
        library_names()
    )
}

pub fn run() -> String {
    "Examples:
  Ask a yes/no question:
    ardana run decider-2b \"I was charged twice for order A-104.\" --noul \"Does the customer ask for a refund?\"

  Pick one of several options, and a level on a scale (lowest first):
    ardana run decider-2b \"The app crashes on login. Fix it now!\" \\
      --choice \"Which team should handle this?\" billing technical sales \\
      --score \"How upset is the customer?\" calm annoyed furious

  Read the state from a file or a pipe:
    cat ticket.txt | ardana run decider-2b --noul \"Is an order number given?\"

  Send a whole /v1/systemone request and print the API's JSON response:
    ardana run decider-2b --request request.json --json

The state comes before the questions. A model that is not pulled yet is downloaded first."
        .to_string()
}

pub fn pull() -> String {
    format!(
        "Examples:
  A library model, or another quantization of it:
    ardana pull decider-2b
    ardana pull decider-2b:q8_0

  Any GGUF repository on Hugging Face (add the tokenizer's repository when the GGUF one has none):
    ardana pull hf.co/Mapika/decider-4b-GGUF
    ardana pull hf.co/ggml-org/SmolLM3-3B-GGUF --tokenizer hf.co/HuggingFaceTB/SmolLM3-3B

  A model already in your Ollama store, or a local file:
    ardana pull ollama:llama3.2
    ardana pull ./model.gguf --tokenizer ./tokenizer.json --name my-model

Library: {}.
Weights download to the Hugging Face cache; Ollama and local files are read where they are.",
        library_names()
    )
}

pub fn list() -> String {
    "Library models that are not pulled yet are downloaded by `ardana pull` or on their first run."
        .to_string()
}

pub fn show() -> String {
    "Examples:
  ardana show decider-2b
  ardana show decider-2b --json
  ardana show decider-4b            A library model not pulled yet"
        .to_string()
}

pub fn ps() -> String {
    "Asks the running `ardana serve` which models it has loaded, most recently used first."
        .to_string()
}

pub fn rm() -> String {
    "Examples:
  ardana rm decider-4b
  ardana rm decider-4b qwen3.5-0.8b

Only the entries are removed: the weights stay in the Hugging Face cache, the Ollama store or where they were."
        .to_string()
}

pub fn serve() -> String {
    "Examples:
  ardana serve                              API and playground on http://127.0.0.1:8000
  ardana serve --host 0.0.0.0 --api-key s3cret
  ardana serve --public                     A public playground that runs no model itself
  curl http://127.0.0.1:8000/v1/systemone -H 'content-type: application/json' -d @request.json

Endpoints: POST /v1/systemone, GET /v1/models, GET /health, and the playground at /.
A request naming a library model that is not pulled yet pulls it first.
With --public, browser models run in each visitor's tab: the server lists the library and serves their files, and
answers every POST /v1/systemone with 403."
        .to_string()
}

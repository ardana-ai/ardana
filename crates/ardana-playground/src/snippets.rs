//! The current request as code: curl, the TypeSafe Python SDK (`typesafe-sdk` 0.7.2) and the TypeSafe TypeScript SDK
//! (`@typesafe-ai/sdk` 0.6.0), each aimed at the server that serves the playground; and, for a model this server does
//! not run (one picked in the tab, or one it has not pulled), the ardana CLI's commands, with ardana.ai's install line
//! or `ardana pull` (Q4).

use ardana_api::SystemOneRequest;
use serde_json::Value;

use crate::request;

/// ardana.ai's install line, for macOS and Linux.
pub const INSTALL: &str = "curl -fsSL https://ardana.ai/install.sh | sh";
/// ardana.ai's install line for Windows, in PowerShell.
pub const INSTALL_WINDOWS: &str = "irm https://ardana.ai/install.ps1 | iex";

/// The ardana CLI's command that pulls `model`, as the pick writes it, into the registry of the machine it runs on: run
/// where the server runs, it adds the model to the server's models.
pub fn pull(model: &str) -> String {
    format!("ardana pull {model}")
}

/// The ardana CLI's command that answers `request` on the model it names (`<model>`, to be filled in, while it names
/// none: the standalone build before anything is picked): `ardana run <model> --request -`, with the
/// exact body Run sends on stdin, in a heredoc as the curl snippet carries it.
pub fn cli(request: &SystemOneRequest) -> String {
    format!(
        "ardana run {} --request - <<'JSON'\n{}\nJSON\n",
        request.model.as_deref().unwrap_or("<model>"),
        request::body(request)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Curl,
    Python,
    TypeScript,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::Curl, Language::Python, Language::TypeScript];

    pub fn name(self) -> &'static str {
        match self {
            Language::Curl => "curl",
            Language::Python => "Python",
            Language::TypeScript => "TypeScript",
        }
    }
}

/// The snippet that sends `request` to the API at `origin`; a request without a model leaves it to the server.
pub fn snippet(language: Language, origin: &str, request: &SystemOneRequest) -> String {
    let origin = origin.trim_end_matches('/');
    let model = |literal: fn(&Value, &str) -> String, line: &str, indent: &str| {
        request.model.as_ref().map_or_else(String::new, |model| {
            line.replace("{}", &literal(&Value::String(model.clone()), indent))
        })
    };
    let questions = Value::Object(request.questions.clone().into_iter().collect());
    match language {
        Language::Curl => format!(
            "curl -sS {origin}/v1/systemone \\\n  -H 'content-type: application/json' \\\n  --data-binary @- <<'JSON'\n{}\nJSON\n",
            request::body(request)
        ),
        Language::Python => format!(
            "# pip install typesafe-sdk==0.7.2\n\
             # Run: TYPESAFE_BASE_URL={origin} TYPESAFE_API_KEY=any python decide.py\n\
             # The SDK needs some key; an Ardana server started without --api-key ignores it.\n\
             from typesafe_sdk import TypeSafeClient\n\
             \n\
             with TypeSafeClient(timeout=120.0) as client:\n\
             \x20   response = client.system_one(\n\
             {}\
             \x20       state={},\n\
             \x20       questions={},\n\
             \x20   )\n\
             \n\
             print(response.model_dump_json(indent=2))\n",
            model(python, "        model={},\n", "        "),
            python(&request.state, "        "),
            python(&questions, "        "),
        ),
        Language::TypeScript => format!(
            "// npm install @typesafe-ai/sdk@0.6.0\n\
             // Run: TYPESAFE_API_KEY=any node decide.mts\n\
             // The SDK needs some key; an Ardana server started without --api-key ignores it.\n\
             import {{ TypeSafeClient }} from \"@typesafe-ai/sdk\";\n\
             \n\
             const client = new TypeSafeClient({{ baseURL: {}, timeout: 120_000 }});\n\
             const response = await client.systemOne({{\n\
             {}\
             \x20 state: {},\n\
             \x20 questions: {},\n\
             }});\n\
             \n\
             console.log(JSON.stringify(response, null, 2));\n",
            json(&Value::String(origin.to_string()), ""),
            model(json, "  model: {},\n", "  "),
            json(&request.state, "  "),
            json(&questions, "  "),
        ),
    }
}

/// Pretty JSON, every line after the first indented by `indent`. JSON strings hold no raw newlines, so indenting by
/// line never touches a string's content.
fn json(value: &Value, indent: &str) -> String {
    serde_json::to_string_pretty(value)
        .expect("JSON values serialise")
        .replace('\n', &format!("\n{indent}"))
}

/// A Python literal of a JSON value: pretty JSON with `null`, `true` and `false` outside strings spelled `None`,
/// `True` and `False`. JSON's string escapes are all valid in Python string literals.
fn python(value: &Value, indent: &str) -> String {
    let text = json(value, indent);
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut rest = text.as_str();
    while let Some(c) = rest.chars().next() {
        if in_string {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
        } else if c == '"' {
            in_string = true;
        } else if let Some((word, python)) =
            [("null", "None"), ("true", "True"), ("false", "False")]
                .into_iter()
                .find(|(word, _)| rest.starts_with(word))
        {
            out.push_str(python);
            rest = &rest[word.len()..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn ticket() -> SystemOneRequest {
        serde_json::from_value(json!({
            "model": "decider:2b",
            "state": {"note": "it's \"null\" and true", "n": null, "ok": false},
            "questions": {"refund": {"type": "noul", "instructions": "Refund?"}}
        }))
        .unwrap()
    }

    #[test]
    fn the_cli_reads_the_exact_body() {
        let text = cli(&ticket());
        let (command, rest) = text.split_once('\n').unwrap();
        assert_eq!(command, "ardana run decider:2b --request - <<'JSON'");
        assert_eq!(
            rest.strip_suffix("\nJSON\n").unwrap(),
            request::body(&ticket())
        );
    }

    #[test]
    fn the_pull_names_the_pick_as_written() {
        assert_eq!(pull("qwen3.5:0.8b"), "ardana pull qwen3.5:0.8b");
        assert_eq!(pull("decider:4b-q8_0"), "ardana pull decider:4b-q8_0");
        assert_eq!(pull("Decider"), "ardana pull Decider");
    }

    #[test]
    fn curl_sends_the_exact_body() {
        let text = snippet(Language::Curl, "http://127.0.0.1:8000/", &ticket());
        let body = text
            .split_once("<<'JSON'\n")
            .unwrap()
            .1
            .strip_suffix("\nJSON\n")
            .unwrap();
        assert_eq!(body, request::body(&ticket()));
        assert!(text.starts_with("curl -sS http://127.0.0.1:8000/v1/systemone \\\n"));
    }

    #[test]
    fn python_literals_keep_strings() {
        assert_eq!(
            python(&ticket().state, ""),
            "{\n  \"note\": \"it's \\\"null\\\" and true\",\n  \"n\": None,\n  \"ok\": False\n}"
        );
        let text = snippet(Language::Python, "http://127.0.0.1:8000", &ticket());
        assert!(text.contains("TYPESAFE_BASE_URL=http://127.0.0.1:8000 "));
        assert!(
            text.contains("    response = client.system_one(\n        model=\"decider:2b\",\n")
        );
        assert!(text.contains("        questions={\n          \"refund\": {\n"));
    }

    #[test]
    fn typescript_names_the_base_url() {
        let text = snippet(Language::TypeScript, "http://127.0.0.1:8000", &ticket());
        assert!(text.contains(
            "new TypeSafeClient({ baseURL: \"http://127.0.0.1:8000\", timeout: 120_000 })"
        ));
        assert!(text.contains(
            "  state: {\n    \"note\": \"it's \\\"null\\\" and true\",\n    \"n\": null,"
        ));
        assert!(text.contains("client.systemOne({\n  model: \"decider:2b\",\n  state: "));
    }

    #[test]
    fn no_model_leaves_it_to_the_server() {
        let request = SystemOneRequest {
            model: None,
            ..ticket()
        };
        let python = snippet(Language::Python, "http://127.0.0.1:8000", &request);
        assert!(
            python.contains("client.system_one(\n        state={"),
            "{python}"
        );
        let typescript = snippet(Language::TypeScript, "http://127.0.0.1:8000", &request);
        assert!(
            typescript.contains("client.systemOne({\n  state: {"),
            "{typescript}"
        );
    }
}

//! Jev share links: `#share/<lz-string compressToEncodedURIComponent(JSON)>` with the payload
//! `{"apiVersion":"v1","documentText","promptsText","selectedModels":["jev-latest"]}`.

use serde::{Deserialize, Serialize};

/// The URL fragment prefix of a share link, after `#`.
pub const PREFIX: &str = "share/";

/// A Jev share payload: the state as text, the `questions` JSON as text and the picked models. Some docs links
/// carry no `selectedModels`; they open on the default model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePayload {
    pub api_version: String,
    pub document_text: String,
    pub prompts_text: String,
    #[serde(default)]
    pub selected_models: Vec<String>,
}

impl SharePayload {
    /// The payload of the playground's editors and picked model (none while no model is picked).
    pub fn new(document_text: String, prompts_text: String, model: String) -> SharePayload {
        SharePayload {
            api_version: "v1".to_string(),
            document_text,
            prompts_text,
            selected_models: (!model.is_empty()).then_some(model).into_iter().collect(),
        }
    }
}

/// The share link for `payload` on the playground at `page` (its URL without query and fragment):
/// `<page>/#share/<encoded payload>`.
pub fn link(page: &str, payload: &SharePayload) -> String {
    let json = serde_json::to_string(payload).expect("a payload of strings serialises");
    format!(
        "{}/#{PREFIX}{}",
        page.trim_end_matches('/'),
        lz_str::compress_to_encoded_uri_component(json.as_str())
    )
}

/// Reads the share in a URL fragment (`location.hash`, with or without `#`): `None` when the fragment is no share
/// link, else the payload or why it cannot be read.
pub fn from_hash(hash: &str) -> Option<Result<SharePayload, String>> {
    let encoded = hash
        .strip_prefix('#')
        .unwrap_or(hash)
        .strip_prefix(PREFIX)?;
    Some(decode(encoded))
}

/// Decodes the part after `#share/`.
pub fn decode(encoded: &str) -> Result<SharePayload, String> {
    let units = lz_str::decompress_from_encoded_uri_component(encoded)
        .filter(|units| !units.is_empty())
        .ok_or("the link's data is not an lz-string payload")?;
    let json = String::from_utf16(&units).map_err(|_| "the link's data is not valid text")?;
    let payload: SharePayload = serde_json::from_str(&json)
        .map_err(|err| format!("the link's data is not a share payload: {err}"))?;
    if payload.api_version != "v1" {
        return Err(format!(
            "the link is for API version {:?}; this playground reads \"v1\"",
            payload.api_version
        ));
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(json: &str) -> String {
        lz_str::compress_to_encoded_uri_component(json)
    }

    #[test]
    fn reads_a_jev_payload() {
        let json = r#"{"apiVersion":"v1","documentText":"Hi","promptsText":"{\"q\":{\"type\":\"noul\",\"instructions\":\"Is it a greeting?\"}}","selectedModels":["jev-latest"]}"#;
        let payload = from_hash(&format!("#share/{}", encode(json)))
            .unwrap()
            .unwrap();
        assert_eq!(payload.document_text, "Hi");
        assert_eq!(payload.selected_models, ["jev-latest"]);
        assert!(payload.prompts_text.contains("greeting"));
    }

    #[test]
    fn a_link_without_a_picked_model_carries_none() {
        let payload = SharePayload::new("Hi".into(), "{}".into(), String::new());
        assert!(payload.selected_models.is_empty());
        let payload = SharePayload::new("Hi".into(), "{}".into(), "decider-2b".into());
        assert_eq!(payload.selected_models, ["decider-2b"]);
    }

    #[test]
    fn a_payload_without_models_opens_on_the_default() {
        let json = r#"{"apiVersion":"v1","documentText":"x","promptsText":"{}"}"#;
        assert_eq!(
            decode(&encode(json)).unwrap().selected_models,
            Vec::<String>::new()
        );
    }

    #[test]
    fn links_round_trip_in_jev_key_order() {
        let payload = SharePayload::new("Café ☕".into(), "{\"q\": 1}".into(), "decider-2b".into());
        let url = link("http://127.0.0.1:8000/", &payload);
        let hash = url.strip_prefix("http://127.0.0.1:8000/").unwrap();
        assert_eq!(from_hash(hash).unwrap().unwrap(), payload);
        let units =
            lz_str::decompress_from_encoded_uri_component(hash.strip_prefix("#share/").unwrap())
                .unwrap();
        assert_eq!(
            String::from_utf16(&units).unwrap(),
            r#"{"apiVersion":"v1","documentText":"Café ☕","promptsText":"{\"q\": 1}","selectedModels":["decider-2b"]}"#
        );
        // The landing's playground keeps its path.
        let url = link("https://ardana.ai/playground/", &payload);
        assert!(
            url.starts_with("https://ardana.ai/playground/#share/"),
            "{url}"
        );
    }

    #[test]
    fn other_fragments_are_no_share() {
        assert_eq!(from_hash(""), None);
        assert_eq!(from_hash("#results"), None);
    }

    #[test]
    fn bad_links_are_errors() {
        assert!(from_hash("#share/").unwrap().is_err());
        assert!(decode("%%%").is_err());
        assert!(decode(&encode("[1, 2]")).is_err());
        let v2 = r#"{"apiVersion":"v2","documentText":"","promptsText":"{}","selectedModels":[]}"#;
        assert!(decode(&encode(v2)).unwrap_err().contains("v2"));
    }
}

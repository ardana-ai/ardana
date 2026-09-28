//! Jev share links: `#share/<lz-string compressToEncodedURIComponent(JSON)>` with the payload
//! `{"apiVersion":"v1","documentText","promptsText","selectedModels":["jev-latest"]}`.

use serde::{Deserialize, Serialize};

/// The URL fragment prefix of a share link, after `#`.
pub const PREFIX: &str = "share/";

/// A Jev share payload: the state as text, the `questions` JSON as text and the picked models.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePayload {
    pub api_version: String,
    pub document_text: String,
    pub prompts_text: String,
    pub selected_models: Vec<String>,
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
        .map_err(|err| format!("the link's data is not a Jev share payload: {err}"))?;
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

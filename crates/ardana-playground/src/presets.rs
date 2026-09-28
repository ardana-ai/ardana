//! The three presets, each a request in the repo's fixture format: helpdesk ticket routing (the `ticket.json` the API
//! suites send), resume screening and a support-chat audit. The resume and the transcript are illustrative samples.

use ardana_api::SystemOneRequest;

use crate::request;

pub struct Preset {
    pub name: &'static str,
    request: &'static str,
}

pub const PRESETS: [Preset; 3] = [
    Preset {
        name: "Ticket routing",
        request: include_str!("../../../tests/fixtures/requests/ticket.json"),
    },
    Preset {
        name: "Resume screening",
        request: include_str!("../presets/resume-screening.json"),
    },
    Preset {
        name: "Support-chat audit",
        request: include_str!("../presets/support-chat-audit.json"),
    },
];

impl Preset {
    /// The state and questions editor texts the preset loads.
    pub fn texts(&self) -> (String, String) {
        let request: SystemOneRequest =
            serde_json::from_str(self.request).expect("the preset requests are valid requests");
        request::editor_texts(&request)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn presets_load_their_requests() {
        let kinds = |preset: &Preset| -> Vec<(String, String)> {
            let questions: serde_json::Map<String, Value> =
                serde_json::from_str(&preset.texts().1).unwrap();
            questions
                .into_iter()
                .map(|(id, q)| (id, q["type"].as_str().unwrap().to_string()))
                .collect()
        };
        let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect()
        };
        assert_eq!(
            kinds(&PRESETS[0]),
            pairs(&[("department", "choice"), ("refund", "noul")])
        );
        assert_eq!(
            kinds(&PRESETS[1]),
            pairs(&[("fit", "score"), ("five_years", "noul")])
        );
        assert_eq!(
            kinds(&PRESETS[2]),
            pairs(&[("apologized", "noul"), ("status", "choice")])
        );

        let (resume, questions) = PRESETS[1].texts();
        assert!(resume.starts_with("SAMPLE RESUME"));
        let fit: Value =
            serde_json::from_str::<Value>(&questions).unwrap()["fit"]["criteria"].clone();
        assert_eq!(fit.as_array().unwrap().len(), 4);

        let (transcript, questions) = PRESETS[2].texts();
        assert!(
            serde_json::from_str::<Value>(&transcript)
                .unwrap()
                .is_object()
        );
        let status: Value =
            serde_json::from_str::<Value>(&questions).unwrap()["status"]["criteria"].clone();
        assert_eq!(
            status.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["resolved", "escalated", "open"]
        );
    }
}

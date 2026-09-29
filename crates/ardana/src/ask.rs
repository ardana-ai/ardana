//! `ardana run`'s inline input: the state, and the `--noul`, `--choice` and `--score` questions in the order they were
//! given, as a `/v1/systemone` request.

use ardana_api::{SystemOneRequest, state_value};
use clap::ArgMatches;
use serde_json::{Value, json};

/// The flag that asked a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Noul,
    Choice,
    Score,
}

/// The questions the flags asked (`--noul QUESTION`, `--choice QUESTION OPTION...`, `--score QUESTION LEVEL...`),
/// grouped per occurrence, and the order they were given in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Asked {
    pub noul: Vec<String>,
    pub choice: Vec<Vec<String>>,
    pub score: Vec<Vec<String>>,
    /// Each flag as (kind, its occurrence among that flag's), in command-line order.
    pub order: Vec<(Kind, usize)>,
}

impl Asked {
    /// The questions in `ardana run`'s matches. clap derives no grouped values, so each flag's occurrences are read
    /// here, and the order from the index of each occurrence's first value.
    pub fn from_matches(matches: &ArgMatches) -> Asked {
        let groups = |id: &str| -> (Vec<Vec<String>>, Vec<usize>) {
            let Some(occurrences) = matches.get_occurrences::<String>(id) else {
                return (Vec::new(), Vec::new());
            };
            let groups: Vec<Vec<String>> = occurrences
                .map(|values| values.cloned().collect())
                .collect();
            let indices: Vec<usize> = matches
                .indices_of(id)
                .map(Iterator::collect)
                .unwrap_or_default();
            let mut starts = Vec::new();
            let mut at = 0;
            for group in &groups {
                starts.extend(indices.get(at).copied());
                at += group.len();
            }
            (groups, starts)
        };
        let (noul, noul_at) = groups("noul");
        let (choice, choice_at) = groups("choice");
        let (score, score_at) = groups("score");
        let mut seen: Vec<(usize, Kind, usize)> = Vec::new();
        for (kind, starts) in [
            (Kind::Noul, noul_at),
            (Kind::Choice, choice_at),
            (Kind::Score, score_at),
        ] {
            seen.extend(
                starts
                    .into_iter()
                    .enumerate()
                    .map(|(n, index)| (index, kind, n)),
            );
        }
        seen.sort_by_key(|(index, _, _)| *index);
        Asked {
            noul: noul.into_iter().flatten().collect(),
            choice,
            score,
            order: seen.into_iter().map(|(_, kind, n)| (kind, n)).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.noul.len() + self.choice.len() + self.score.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The request: the state as typed (JSON when it parses as an object or array) and the questions as `q1`, `q2`,
    /// ... in their order, or noul, choice, score order when the order does not cover them all.
    pub fn request(&self, state: &str) -> SystemOneRequest {
        let order: Vec<(Kind, usize)> = if self.order.len() == self.len() {
            self.order.clone()
        } else {
            (0..self.noul.len())
                .map(|n| (Kind::Noul, n))
                .chain((0..self.choice.len()).map(|n| (Kind::Choice, n)))
                .chain((0..self.score.len()).map(|n| (Kind::Score, n)))
                .collect()
        };
        let questions = order
            .iter()
            .enumerate()
            .map(|(i, &(kind, n))| (format!("q{}", i + 1), self.spec(kind, n)))
            .collect();
        SystemOneRequest {
            model: None,
            state: state_value(state),
            questions,
        }
    }

    fn spec(&self, kind: Kind, n: usize) -> Value {
        let split = |values: &[String]| match values.split_first() {
            Some((question, rest)) => (question.clone(), rest.to_vec()),
            None => (String::new(), Vec::new()),
        };
        match kind {
            Kind::Noul => json!({ "type": "noul", "instructions": self.noul[n] }),
            Kind::Choice => {
                let (question, options) = split(&self.choice[n]);
                json!({ "type": "choice", "instructions": question, "criteria": options })
            }
            Kind::Score => {
                let (question, levels) = split(&self.score[n]);
                json!({ "type": "score", "instructions": question, "criteria": levels })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn questions_become_specs_in_the_given_order() {
        let mut asked = Asked {
            noul: strings(&["Does the customer ask for a refund?"]),
            choice: vec![strings(&["Which team?", "billing", "technical"])],
            score: vec![strings(&["How upset?", "calm", "furious"])],
            order: vec![(Kind::Choice, 0), (Kind::Noul, 0), (Kind::Score, 0)],
        };
        let built = asked.request("{\"id\": 7}");
        assert_eq!(built.state, json!({ "id": 7 }));
        assert_eq!(
            serde_json::to_value(&built.questions).unwrap(),
            json!({
                "q1": { "type": "choice", "instructions": "Which team?", "criteria": ["billing", "technical"] },
                "q2": { "type": "noul", "instructions": "Does the customer ask for a refund?" },
                "q3": { "type": "score", "instructions": "How upset?", "criteria": ["calm", "furious"] },
            })
        );
        // Without an order covering every question: noul, choice, score.
        asked.order.clear();
        let built = asked.request("text");
        let kinds: Vec<&str> = built
            .questions
            .values()
            .map(|q| q["type"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["noul", "choice", "score"]);
        assert_eq!(built.state, json!("text"));
    }
}

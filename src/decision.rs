use crate::contracts::{CONTRACT_VERSION, Decision, DecisionBundle, QuestionBundle};
use std::collections::{BTreeMap, HashSet};

fn tokens(text: &str) -> HashSet<String> {
    text.to_ascii_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.len() > 1)
        .map(str::to_owned)
        .collect()
}

pub fn heuristic(bundle: &QuestionBundle) -> DecisionBundle {
    let instruction = tokens(&bundle.user_instruction);
    let decisions = bundle
        .questions
        .iter()
        .map(|question| {
            let mut scored: Vec<(String, f64)> = question
                .criteria
                .iter()
                .map(|criterion| {
                    let overlap =
                        tokens(&criterion.label).intersection(&instruction).count() as f64;
                    let id_bonus =
                        instruction.contains(&criterion.id.to_ascii_lowercase()) as u8 as f64;
                    (criterion.id.clone(), overlap + id_bonus)
                })
                .collect();
            scored.sort_by(|a, b| b.1.total_cmp(&a.1));
            let max = scored.first().map(|entry| entry.1).unwrap_or(0.0);
            let weights: Vec<f64> = scored
                .iter()
                .map(|(_, score)| {
                    if max == 0.0 {
                        1.0
                    } else {
                        (score + 1.0).powi(3)
                    }
                })
                .collect();
            let total: f64 = weights.iter().sum();
            let probabilities: BTreeMap<String, f64> = scored
                .iter()
                .zip(weights)
                .map(|((id, _), weight)| (id.clone(), weight / total))
                .collect();
            let choice = if max == 0.0 {
                if probabilities.contains_key("none") {
                    "none".into()
                } else {
                    scored[0].0.clone()
                }
            } else {
                scored[0].0.clone()
            };
            Decision {
                question_id: question.id.clone(),
                choice,
                probabilities,
            }
        })
        .collect();
    DecisionBundle {
        contract: CONTRACT_VERSION.into(),
        snapshot_id: bundle.snapshot_id.clone(),
        engine: "local-heuristic-v1".into(),
        decisions,
    }
}

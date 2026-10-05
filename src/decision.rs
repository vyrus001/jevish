use crate::contracts::{CONTRACT_VERSION, Decision, DecisionBundle, QuestionBundle};
use std::collections::{BTreeMap, HashSet};

fn tokens(text: &str) -> HashSet<String> {
    text.to_ascii_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.len() > 1)
        .map(str::to_owned)
        .collect()
}

fn ordered_tokens(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn accessible_name(label: &str) -> Option<String> {
    let start = label.find('"')?;
    serde_json::Deserializer::from_str(&label[start..])
        .into_iter::<String>()
        .next()?
        .ok()
}

fn exact_target(question: &crate::contracts::Question, instruction: &str) -> Option<String> {
    let instruction = ordered_tokens(instruction);
    let mut matches = question
        .criteria
        .iter()
        .filter(|criterion| !matches!(criterion.id.as_str(), "none" | "ambiguous"))
        .filter_map(|criterion| {
            let name = ordered_tokens(&accessible_name(&criterion.label)?);
            (!name.is_empty()
                && instruction
                    .windows(name.len())
                    .any(|window| window == name.as_slice()))
            .then_some((criterion.id.as_str(), name.len()))
        })
        .collect::<Vec<_>>();
    let longest = matches.iter().map(|(_, length)| *length).max()?;
    matches.retain(|(_, length)| *length == longest);
    match matches.as_slice() {
        [(id, _)] => Some((*id).to_owned()),
        _ => Some("ambiguous".into()),
    }
}

pub fn heuristic(bundle: &QuestionBundle) -> DecisionBundle {
    let instruction = tokens(&bundle.user_instruction);
    let decisions = bundle
        .questions
        .iter()
        .map(|question| {
            if question.id == "target"
                && let Some(choice) = exact_target(question, &bundle.user_instruction)
            {
                let probabilities = question
                    .criteria
                    .iter()
                    .map(|criterion| {
                        let probability = (criterion.id == choice) as u8 as f64;
                        (criterion.id.clone(), probability)
                    })
                    .collect();
                return Decision {
                    question_id: question.id.clone(),
                    choice,
                    probabilities,
                };
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{Criterion, Question};

    fn target_question(labels: &[(&str, &str)]) -> Question {
        Question {
            id: "target".into(),
            instruction: String::new(),
            criteria: labels
                .iter()
                .map(|(id, label)| Criterion {
                    id: (*id).into(),
                    label: (*label).into(),
                })
                .chain([
                    Criterion {
                        id: "none".into(),
                        label: "No matching element".into(),
                    },
                    Criterion {
                        id: "ambiguous".into(),
                        label: "Several elements match equally".into(),
                    },
                ])
                .collect(),
        }
    }

    #[test]
    fn selects_unique_exact_accessible_name_with_full_confidence() {
        let question = target_question(&[
            ("e1", "button \"Continue to sign in\""),
            ("e2", "button \"Cancel\""),
        ]);
        let bundle = QuestionBundle {
            contract: CONTRACT_VERSION.into(),
            snapshot_id: "s".into(),
            user_instruction: "click Continue to sign in".into(),
            questions: vec![question],
        };
        let decision = heuristic(&bundle).decisions.remove(0);
        assert_eq!(decision.choice, "e1");
        assert_eq!(decision.probabilities["e1"], 1.0);
        assert_eq!(decision.probabilities["none"], 0.0);
    }

    #[test]
    fn exact_name_beats_longer_prefix_label() {
        let question = target_question(&[
            ("e1", "button \"Sign in\""),
            ("e2", "button \"Sign in using root user email\""),
        ]);
        assert_eq!(exact_target(&question, "click Sign in"), Some("e1".into()));
    }

    #[test]
    fn duplicate_exact_names_remain_ambiguous() {
        let question = target_question(&[
            ("e1", "button \"Sign in\" in region: Primary"),
            ("e2", "button \"Sign in\" in region: Secondary"),
        ]);
        assert_eq!(
            exact_target(&question, "click Sign in"),
            Some("ambiguous".into())
        );
    }
}

use crate::contracts::{
    ActionPlan, Binding, CONTRACT_VERSION, CandidateSet, Decision, DecisionBundle, Operation,
    QuestionBundle, TargetGuard,
};
use anyhow::{Result, bail};
use std::collections::BTreeSet;

fn accepted<'a>(
    decision: &'a Decision,
    criteria: &BTreeSet<&str>,
    question: &str,
    probability: f64,
    margin: f64,
) -> Result<(&'a str, f64, f64)> {
    if decision.question_id != question {
        bail!("decision question mismatch: expected {question}")
    }
    let probability_ids = decision
        .probabilities
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if probability_ids != *criteria {
        bail!("decision probabilities do not match question criteria")
    }
    if decision
        .probabilities
        .values()
        .any(|value| !value.is_finite() || *value < 0.0 || *value > 1.0)
    {
        bail!("probabilities must be finite and between zero and one")
    }
    let selected = *decision
        .probabilities
        .get(&decision.choice)
        .ok_or_else(|| anyhow::anyhow!("choice missing from probabilities"))?;
    let total: f64 = decision.probabilities.values().sum();
    if (total - 1.0).abs() > 0.025 {
        bail!("probabilities must sum to one")
    }
    let second = decision
        .probabilities
        .iter()
        .filter(|(id, _)| *id != &decision.choice)
        .map(|(_, value)| *value)
        .fold(0.0, f64::max);
    let gap = selected - second;
    if matches!(decision.choice.as_str(), "none" | "ambiguous") {
        bail!("decision returned {}", decision.choice)
    }
    if selected < probability || gap < margin {
        bail!("decision below gate: probability={selected:.3}, margin={gap:.3}")
    }
    Ok((&decision.choice, selected, gap))
}

pub fn build_plan(
    questions: &QuestionBundle,
    decisions: &DecisionBundle,
    candidates: &CandidateSet,
    binding: &Binding,
    probability: f64,
    margin: f64,
) -> Result<ActionPlan> {
    if questions.snapshot_id != decisions.snapshot_id
        || candidates.snapshot_id != decisions.snapshot_id
    {
        bail!("snapshot IDs do not match")
    }
    let operation = if let Some(operation) = binding.operation {
        operation
    } else {
        let decision = decisions
            .decisions
            .iter()
            .find(|decision| decision.question_id == "operation")
            .ok_or_else(|| anyhow::anyhow!("missing operation decision"))?;
        let criteria = question_criteria(questions, "operation")?;
        let (choice, _, _) = accepted(decision, &criteria, "operation", probability, margin)?;
        serde_json::from_value(serde_json::Value::String(choice.into()))?
    };
    if operation != candidates.operation {
        bail!("candidate operation does not match selected operation")
    }
    let mut confidence: f64 = 1.0;
    let mut gap: f64 = 1.0;
    let target = if operation.needs_target() {
        let decision = decisions
            .decisions
            .iter()
            .find(|decision| decision.question_id == "target")
            .ok_or_else(|| anyhow::anyhow!("missing target decision"))?;
        let criteria = question_criteria(questions, "target")?;
        let (choice, selected, selected_gap) =
            accepted(decision, &criteria, "target", probability, margin)?;
        confidence = confidence.min(selected);
        gap = gap.min(selected_gap);
        let element = candidates
            .candidates
            .iter()
            .find(|element| element.id == choice)
            .ok_or_else(|| anyhow::anyhow!("selected target is not a candidate"))?;
        Some(TargetGuard {
            element_id: element.id.clone(),
            fingerprint: element.fingerprint.clone(),
            backend_node_id: element.backend_node_id,
        })
    } else {
        None
    };
    let value = if operation.needs_value() {
        let choices = match operation {
            Operation::Open => &binding.urls,
            Operation::Press => &binding.keys,
            Operation::Scroll => &binding.directions,
            _ => &binding.values,
        };
        match choices.len() {
            0 => bail!("operation requires an explicit value"),
            1 => Some(choices[0].clone()),
            _ => {
                let decision = decisions
                    .decisions
                    .iter()
                    .find(|decision| decision.question_id == "value")
                    .ok_or_else(|| anyhow::anyhow!("missing value decision"))?;
                let (choice, selected, selected_gap) = accepted(
                    decision,
                    &question_criteria(questions, "value")?,
                    "value",
                    probability,
                    margin,
                )?;
                confidence = confidence.min(selected);
                gap = gap.min(selected_gap);
                let index: usize = choice
                    .strip_prefix('v')
                    .ok_or_else(|| anyhow::anyhow!("invalid value choice"))?
                    .parse()?;
                Some(
                    choices
                        .get(index)
                        .ok_or_else(|| anyhow::anyhow!("value choice out of range"))?
                        .clone(),
                )
            }
        }
    } else {
        None
    };
    Ok(ActionPlan {
        contract: CONTRACT_VERSION.into(),
        snapshot_id: candidates.snapshot_id.clone(),
        document: candidates.document.clone(),
        operation,
        target,
        value,
        confidence,
        margin: gap,
    })
}

fn question_criteria<'a>(questions: &'a QuestionBundle, id: &str) -> Result<BTreeSet<&'a str>> {
    questions
        .questions
        .iter()
        .find(|question| question.id == id)
        .map(|question| {
            question
                .criteria
                .iter()
                .map(|item| item.id.as_str())
                .collect()
        })
        .ok_or_else(|| anyhow::anyhow!("missing {id} question"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{DocumentIdentity, Element};
    use std::collections::BTreeMap;

    fn element(id: &str, name: &str, backend_node_id: i64) -> Element {
        Element {
            id: id.into(),
            backend_node_id: Some(backend_node_id),
            role: "button".into(),
            name: name.into(),
            description: String::new(),
            value: String::new(),
            context: vec![],
            states: BTreeMap::new(),
            attributes: BTreeMap::new(),
            fingerprint: format!("fingerprint-{id}"),
        }
    }

    #[test]
    fn exact_name_decision_passes_default_gate_without_lowering_thresholds() {
        let document = DocumentIdentity {
            url: "https://example.test".into(),
            title: "Sign in".into(),
            context_id: "frame".into(),
            revision: "loader".into(),
        };
        let candidates = CandidateSet {
            contract: CONTRACT_VERSION.into(),
            snapshot_id: "snapshot".into(),
            document,
            operation: Operation::Click,
            candidates: vec![
                element("e1", "Sign in", 1),
                element("e2", "Sign in using root user email", 2),
            ],
        };
        let binding = Binding {
            contract: CONTRACT_VERSION.into(),
            instruction: "click Sign in".into(),
            operation: Some(Operation::Click),
            values: vec![],
            urls: vec![],
            keys: vec![],
            directions: vec![],
        };
        let questions = crate::questions::build(&binding, Some(&candidates));
        let decisions = crate::decision::heuristic(&questions);
        let plan = build_plan(&questions, &decisions, &candidates, &binding, 0.8, 0.2)
            .expect("unique exact name must pass default gate");
        assert_eq!(plan.target.unwrap().element_id, "e1");
        assert_eq!(plan.confidence, 1.0);
        assert_eq!(plan.margin, 1.0);
    }
}

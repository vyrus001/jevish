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
        bail!("GATE_INVALID_DECISION: decision question does not match expected {question}")
    }
    let probability_ids = decision
        .probabilities
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if probability_ids.is_empty() || !probability_ids.is_subset(criteria) {
        bail!(
            "GATE_INVALID_DECISION: {question} probabilities contain no known choices or unknown choice IDs"
        )
    }
    if decision
        .probabilities
        .values()
        .any(|value| !value.is_finite() || *value < 0.0 || *value > 1.0)
    {
        bail!(
            "GATE_INVALID_PROBABILITY: {question} probabilities must be finite and between zero and one"
        )
    }
    let selected = *decision
        .probabilities
        .get(&decision.choice)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "GATE_INVALID_DECISION: selected {question} choice is missing from probabilities"
            )
        })?;
    let total: f64 = decision.probabilities.values().sum();
    if (total - 1.0).abs() > 0.025 {
        bail!("GATE_INVALID_PROBABILITY: {question} probabilities must sum to one")
    }
    let second = decision
        .probabilities
        .iter()
        .filter(|(id, _)| *id != &decision.choice)
        .map(|(_, value)| *value)
        .fold(0.0, f64::max);
    let gap = selected - second;
    if matches!(decision.choice.as_str(), "none" | "ambiguous") {
        bail!(
            "GATE_NO_TARGET: {question} decision returned {}",
            decision.choice
        )
    }
    if selected < probability || gap < margin {
        bail!("GATE_LOW_CONFIDENCE: {question} probability={selected:.3}, margin={gap:.3}")
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
        bail!("GATE_STALE_SNAPSHOT: snapshot IDs do not match")
    }
    let operation = if let Some(operation) = binding.operation {
        operation
    } else {
        let decision = decisions
            .decisions
            .iter()
            .find(|decision| decision.question_id == "operation")
            .ok_or_else(|| anyhow::anyhow!("GATE_MISSING_DECISION: missing operation decision"))?;
        let criteria = question_criteria(questions, "operation")?;
        let (choice, _, _) = accepted(decision, &criteria, "operation", probability, margin)?;
        serde_json::from_value(serde_json::Value::String(choice.into()))?
    };
    if operation != candidates.operation {
        bail!("GATE_OPERATION_MISMATCH: candidate operation does not match selected operation")
    }
    let mut confidence: f64 = 1.0;
    let mut gap: f64 = 1.0;
    let target = if operation.needs_target() {
        let decision = decisions
            .decisions
            .iter()
            .find(|decision| decision.question_id == "target")
            .ok_or_else(|| anyhow::anyhow!("GATE_MISSING_DECISION: missing target decision"))?;
        let criteria = question_criteria(questions, "target")?;
        let (choice, selected, selected_gap) =
            accepted(decision, &criteria, "target", probability, margin)?;
        confidence = confidence.min(selected);
        gap = gap.min(selected_gap);
        let element = candidates
            .candidates
            .iter()
            .find(|element| element.id == choice)
            .ok_or_else(|| {
                anyhow::anyhow!("GATE_INVALID_TARGET: selected target is not a candidate")
            })?;
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
            0 => bail!("GATE_MISSING_VALUE: operation requires an explicit value"),
            1 => Some(choices[0].clone()),
            _ => {
                let decision = decisions
                    .decisions
                    .iter()
                    .find(|decision| decision.question_id == "value")
                    .ok_or_else(|| {
                        anyhow::anyhow!("GATE_MISSING_DECISION: missing value decision")
                    })?;
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
                    .ok_or_else(|| {
                        anyhow::anyhow!("GATE_INVALID_VALUE_CHOICE: invalid value choice")
                    })?
                    .parse()?;
                Some(
                    choices
                        .get(index)
                        .ok_or_else(|| {
                            anyhow::anyhow!("GATE_INVALID_VALUE_CHOICE: value choice out of range")
                        })?
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
        .ok_or_else(|| anyhow::anyhow!("GATE_MISSING_QUESTION: missing {id} question"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{DocumentIdentity, Element};
    use std::collections::BTreeMap;

    fn element(id: &str, role: &str, name: &str, backend_node_id: i64) -> Element {
        Element {
            id: id.into(),
            backend_node_id: Some(backend_node_id),
            role: role.into(),
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
                element("e1", "button", "Sign in", 1),
                element("e2", "button", "Sign in using root user email", 2),
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

    #[test]
    fn sparse_exact_aws_account_decision_passes_gate_without_exposing_value() {
        let document = DocumentIdentity {
            url: "https://signin.aws.amazon.com/".into(),
            title: "AWS sign-in".into(),
            context_id: "frame".into(),
            revision: "loader".into(),
        };
        let candidates = CandidateSet {
            contract: CONTRACT_VERSION.into(),
            snapshot_id: "snapshot".into(),
            document,
            operation: Operation::Fill,
            candidates: vec![element(
                "e7",
                "textbox",
                "Account ID or alias (Don't have?)",
                7,
            )],
        };
        let secret_test_value = "sensitive-test-account";
        let binding = Binding {
            contract: CONTRACT_VERSION.into(),
            instruction: "Fill AWS account ID or alias".into(),
            operation: Some(Operation::Fill),
            values: vec![secret_test_value.into()],
            urls: vec![],
            keys: vec![],
            directions: vec![],
        };
        let questions = crate::questions::build(&binding, Some(&candidates));
        let decisions = DecisionBundle {
            contract: CONTRACT_VERSION.into(),
            snapshot_id: "snapshot".into(),
            engine: "exact-name-filter".into(),
            decisions: vec![Decision {
                question_id: "target".into(),
                choice: "e7".into(),
                probabilities: BTreeMap::from([("e7".into(), 1.0)]),
            }],
        };

        let plan = build_plan(&questions, &decisions, &candidates, &binding, 0.8, 0.2)
            .expect("sparse exact-name decision must pass the gate");
        assert_eq!(plan.target.unwrap().element_id, "e7");
        assert_eq!(plan.value.as_deref(), Some(secret_test_value));
        assert_eq!(plan.confidence, 1.0);
        assert_eq!(plan.margin, 1.0);
    }

    #[test]
    fn gate_diagnostic_does_not_include_bound_value_or_url() {
        let criteria = BTreeSet::from(["e1", "none", "ambiguous"]);
        let untrusted_choice = "credential-shaped-choice";
        let decision = Decision {
            question_id: "target".into(),
            choice: untrusted_choice.into(),
            probabilities: BTreeMap::from([(untrusted_choice.into(), 1.0)]),
        };
        let diagnostic = accepted(&decision, &criteria, "target", 0.8, 0.2)
            .expect_err("unknown choice must fail")
            .to_string();
        assert!(diagnostic.starts_with("GATE_INVALID_DECISION:"));
        assert!(!diagnostic.contains(untrusted_choice));
        assert!(!diagnostic.contains("https://"));
    }
}

use crate::contracts::{
    ActionPlan, Binding, CONTRACT_VERSION, CandidateSet, Decision, DecisionBundle, Operation,
    QuestionBundle, TargetGuard,
};
use anyhow::{Result, bail};

fn accepted<'a>(
    decision: &'a Decision,
    question: &str,
    probability: f64,
    margin: f64,
) -> Result<(&'a str, f64, f64)> {
    if decision.question_id != question {
        bail!("decision question mismatch: expected {question}")
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
        let (choice, _, _) = accepted(decision, "operation", probability, margin)?;
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
        let (choice, selected, selected_gap) = accepted(decision, "target", probability, margin)?;
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
                let (choice, selected, selected_gap) =
                    accepted(decision, "value", probability, margin)?;
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

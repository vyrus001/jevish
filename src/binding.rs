use crate::contracts::{Binding, CONTRACT_VERSION, Operation};
use regex::Regex;

fn infer_operation(lower: &str) -> Option<Operation> {
    let rules = [
        (Operation::Uncheck, &["uncheck", "clear checkbox"][..]),
        (Operation::Check, &["check", "tick", "enable"][..]),
        (Operation::Fill, &["fill", "replace", "set field"][..]),
        (Operation::Type, &["type", "enter text", "append"][..]),
        (
            Operation::Click,
            &["click", "press button", "follow link"][..],
        ),
        (Operation::Select, &["select", "choose option"][..]),
        (Operation::GetText, &["read", "get text", "what does"][..]),
        (Operation::Open, &["open", "navigate", "go to"][..]),
        (Operation::Scroll, &["scroll"][..]),
        (Operation::Press, &["press", "hit key"][..]),
        (Operation::Focus, &["focus"][..]),
    ];
    rules.into_iter().find_map(|(operation, terms)| {
        terms
            .iter()
            .any(|term| lower.contains(term))
            .then_some(operation)
    })
}

pub fn bind(instruction: &str, explicit_operation: Option<Operation>) -> Binding {
    let quote = Regex::new(r#"[\"']([^\"']+)[\"']"#).expect("static regex");
    let url = Regex::new(r#"https?://[^\s\"'<>]+"#).expect("static regex");
    let values = quote
        .captures_iter(instruction)
        .filter_map(|capture| capture.get(1).map(|value| value.as_str().to_owned()))
        .collect();
    let urls = url
        .find_iter(instruction)
        .map(|value| value.as_str().trim_end_matches(['.', ',', ')']).to_owned())
        .collect();
    let lower = instruction.to_ascii_lowercase();
    let keys = [
        "enter",
        "tab",
        "escape",
        "backspace",
        "arrowup",
        "arrowdown",
        "arrowleft",
        "arrowright",
    ]
    .into_iter()
    .filter(|key| lower.contains(key))
    .map(str::to_owned)
    .collect();
    let directions = ["up", "down", "left", "right", "top", "bottom"]
        .into_iter()
        .filter(|direction| lower.split_whitespace().any(|word| word == *direction))
        .map(str::to_owned)
        .collect();
    Binding {
        contract: CONTRACT_VERSION.into(),
        instruction: instruction.into(),
        operation: explicit_operation.or_else(|| infer_operation(&lower)),
        values,
        urls,
        keys,
        directions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_without_model() {
        let result = bind("fill the Name field with 'Ada Lovelace'", None);
        assert_eq!(result.operation, Some(Operation::Fill));
        assert_eq!(result.values, vec!["Ada Lovelace"]);
    }
}

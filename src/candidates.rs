use crate::contracts::{CONTRACT_VERSION, CandidateSet, Element, ElementSnapshot, Operation};

fn allowed(operation: Operation, element: &Element) -> bool {
    let role = element.role.as_str();
    if element.states.get("disabled") == Some(&true) {
        return false;
    }
    if matches!(
        operation,
        Operation::Fill | Operation::Type | Operation::Select
    ) && element.states.get("readonly") == Some(&true)
    {
        return false;
    }
    match operation {
        Operation::Fill => matches!(
            role,
            "textbox" | "searchbox" | "combobox" | "spinbutton" | "date" | "datetime"
        ),
        Operation::Type => matches!(role, "textbox" | "searchbox" | "combobox" | "spinbutton"),
        Operation::Check | Operation::Uncheck => {
            matches!(role, "checkbox" | "switch" | "menuitemcheckbox")
        }
        Operation::Select => matches!(role, "combobox" | "listbox" | "option"),
        Operation::Click => matches!(
            role,
            "button"
                | "link"
                | "checkbox"
                | "radio"
                | "switch"
                | "menuitem"
                | "menuitemcheckbox"
                | "menuitemradio"
                | "option"
                | "tab"
                | "treeitem"
        ),
        Operation::GetText => matches!(
            role,
            "statictext" | "heading" | "region" | "button" | "link"
        ),
        Operation::Focus => element.states.get("focusable").copied().unwrap_or(matches!(
            role,
            "textbox" | "searchbox" | "combobox" | "button" | "link"
        )),
        Operation::Press | Operation::Scroll | Operation::Open => false,
    }
}

pub fn candidates(snapshot: &ElementSnapshot, operation: Operation) -> CandidateSet {
    CandidateSet {
        contract: CONTRACT_VERSION.into(),
        snapshot_id: snapshot.snapshot_id.clone(),
        document: snapshot.document.clone(),
        operation,
        candidates: snapshot
            .elements
            .iter()
            .filter(|element| allowed(operation, element))
            .cloned()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{DocumentIdentity, ElementSnapshot};
    use std::collections::BTreeMap;

    fn element(role: &str, disabled: bool, readonly: bool) -> Element {
        Element {
            id: role.into(),
            backend_node_id: Some(1),
            role: role.into(),
            name: "Target".into(),
            description: String::new(),
            value: String::new(),
            context: vec![],
            states: BTreeMap::from([("disabled".into(), disabled), ("readonly".into(), readonly)]),
            attributes: BTreeMap::new(),
            fingerprint: "f".into(),
        }
    }

    #[test]
    fn excludes_disabled_and_readonly_action_targets() {
        let snapshot = ElementSnapshot {
            contract: CONTRACT_VERSION.into(),
            backend: "test".into(),
            snapshot_id: "s".into(),
            document: DocumentIdentity {
                url: "u".into(),
                title: "t".into(),
                context_id: "c".into(),
                revision: "r".into(),
            },
            captured_at_ms: 0,
            elements: vec![
                element("button", true, false),
                element("textbox", false, true),
                element("textbox", false, false),
            ],
        };
        assert!(
            candidates(&snapshot, Operation::Click)
                .candidates
                .is_empty()
        );
        assert_eq!(candidates(&snapshot, Operation::Fill).candidates.len(), 1);
    }
}

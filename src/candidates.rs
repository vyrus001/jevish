use crate::contracts::{CONTRACT_VERSION, CandidateSet, Element, ElementSnapshot, Operation};

fn allowed(operation: Operation, element: &Element) -> bool {
    let role = element.role.as_str();
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

use crate::contracts::{CONTRACT_VERSION, Element, ElementSnapshot, RawNode, RawSnapshot};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

const ACTIONABLE: &[&str] = &[
    "button",
    "link",
    "textbox",
    "searchbox",
    "combobox",
    "checkbox",
    "radio",
    "switch",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "listbox",
    "spinbutton",
    "slider",
    "tab",
    "treeitem",
    "date",
    "datetime",
    "statictext",
    "heading",
    "region",
];

fn hash(parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part.as_bytes());
        digest.update([0]);
    }
    format!("{:x}", digest.finalize())[..16].to_string()
}

fn bool_property(node: &RawNode, key: &str) -> Option<bool> {
    node.properties.get(key).and_then(|value| {
        value
            .as_bool()
            .or_else(|| value.get("value").and_then(serde_json::Value::as_bool))
    })
}

fn string_property(node: &RawNode, key: &str) -> Option<String> {
    node.properties.get(key).and_then(|value| {
        value.as_str().map(str::to_owned).or_else(|| {
            value
                .get("value")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
    })
}

pub fn extract(raw: &RawSnapshot) -> ElementSnapshot {
    let nodes: HashMap<&str, &RawNode> = raw
        .nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node))
        .collect();
    let mut seen = HashSet::new();
    let mut elements = Vec::new();

    for node in &raw.nodes {
        let role = node.role.to_ascii_lowercase();
        if node.ignored
            || !ACTIONABLE.contains(&role.as_str())
            || (node.name.is_empty() && node.value.is_empty() && role == "statictext")
        {
            continue;
        }

        let mut context = Vec::new();
        let mut parent = node.parent_id.as_deref();
        while let Some(parent_id) = parent {
            let Some(ancestor) = nodes.get(parent_id) else {
                break;
            };
            let label = if ancestor.name.is_empty() {
                ancestor.role.clone()
            } else {
                format!("{}: {}", ancestor.role, ancestor.name)
            };
            if !label.is_empty() && context.last() != Some(&label) {
                context.push(label);
            }
            parent = ancestor.parent_id.as_deref();
            if context.len() >= 5 {
                break;
            }
        }
        context.reverse();

        let backend = node
            .backend_node_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| node.node_id.clone());
        let fingerprint = hash(&[
            &backend,
            &role,
            &node.name,
            &node.description,
            &context.join(" > "),
        ]);
        if !seen.insert(fingerprint.clone()) {
            continue;
        }

        let mut states = BTreeMap::new();
        for key in [
            "disabled",
            "focusable",
            "focused",
            "selected",
            "checked",
            "expanded",
            "required",
            "readonly",
        ] {
            if let Some(value) = bool_property(node, key) {
                states.insert(key.to_string(), value);
            }
        }
        let mut attributes = BTreeMap::new();
        for key in [
            "placeholder",
            "autocomplete",
            "haspopup",
            "valuemin",
            "valuemax",
            "valuetext",
            "keyshortcuts",
        ] {
            if let Some(value) = string_property(node, key) {
                attributes.insert(key.to_string(), value);
            }
        }
        elements.push(Element {
            id: format!("e{}", elements.len() + 1),
            backend_node_id: node.backend_node_id,
            role,
            name: node.name.clone(),
            description: node.description.clone(),
            value: node.value.clone(),
            context,
            states,
            attributes,
            fingerprint,
        });
    }

    ElementSnapshot {
        contract: CONTRACT_VERSION.into(),
        backend: raw.backend.clone(),
        snapshot_id: raw.snapshot_id.clone(),
        document: raw.document.clone(),
        captured_at_ms: raw.captured_at_ms,
        elements,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{DocumentIdentity, RawSnapshot};

    #[test]
    fn extracts_context_and_deduplicates() {
        let raw = RawSnapshot {
            contract: CONTRACT_VERSION.into(),
            backend: "test".into(),
            snapshot_id: "s".into(),
            document: DocumentIdentity {
                url: "https://example.test".into(),
                title: "x".into(),
                context_id: "c".into(),
                revision: "r".into(),
            },
            captured_at_ms: 0,
            nodes: vec![
                RawNode {
                    node_id: "1".into(),
                    parent_id: None,
                    backend_node_id: Some(1),
                    ignored: false,
                    role: "region".into(),
                    name: "Checkout".into(),
                    description: "".into(),
                    value: "".into(),
                    properties: BTreeMap::new(),
                },
                RawNode {
                    node_id: "2".into(),
                    parent_id: Some("1".into()),
                    backend_node_id: Some(2),
                    ignored: false,
                    role: "button".into(),
                    name: "Pay".into(),
                    description: "".into(),
                    value: "".into(),
                    properties: BTreeMap::new(),
                },
            ],
        };
        let output = extract(&raw);
        assert_eq!(output.elements[1].context, vec!["region: Checkout"]);
        assert_eq!(output.elements[1].id, "e2");
    }
}

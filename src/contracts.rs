use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONTRACT_VERSION: &str = "jevish.browser/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentIdentity {
    pub url: String,
    pub title: String,
    pub context_id: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawNode {
    pub node_id: String,
    pub parent_id: Option<String>,
    pub backend_node_id: Option<i64>,
    pub ignored: bool,
    pub role: String,
    pub name: String,
    pub description: String,
    pub value: String,
    #[serde(default)]
    pub properties: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawSnapshot {
    pub contract: String,
    pub backend: String,
    pub snapshot_id: String,
    pub document: DocumentIdentity,
    pub captured_at_ms: u128,
    pub nodes: Vec<RawNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Element {
    pub id: String,
    pub backend_node_id: Option<i64>,
    pub role: String,
    pub name: String,
    pub description: String,
    pub value: String,
    pub context: Vec<String>,
    #[serde(default)]
    pub states: BTreeMap<String, bool>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElementSnapshot {
    pub contract: String,
    pub backend: String,
    pub snapshot_id: String,
    pub document: DocumentIdentity,
    pub captured_at_ms: u128,
    pub elements: Vec<Element>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Click,
    Fill,
    Type,
    Check,
    Uncheck,
    Select,
    GetText,
    Focus,
    Press,
    Scroll,
    Open,
}

impl Operation {
    pub fn needs_target(self) -> bool {
        !matches!(self, Self::Press | Self::Scroll | Self::Open)
    }

    pub fn needs_value(self) -> bool {
        matches!(
            self,
            Self::Fill | Self::Type | Self::Select | Self::Press | Self::Scroll | Self::Open
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CandidateSet {
    pub contract: String,
    pub snapshot_id: String,
    pub document: DocumentIdentity,
    pub operation: Operation,
    pub candidates: Vec<Element>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Binding {
    pub contract: String,
    pub instruction: String,
    pub operation: Option<Operation>,
    pub values: Vec<String>,
    pub urls: Vec<String>,
    pub keys: Vec<String>,
    pub directions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Criterion {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Question {
    pub id: String,
    pub instruction: String,
    pub criteria: Vec<Criterion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuestionBundle {
    pub contract: String,
    pub snapshot_id: String,
    pub user_instruction: String,
    pub questions: Vec<Question>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Decision {
    pub question_id: String,
    pub choice: String,
    pub probabilities: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionBundle {
    pub contract: String,
    pub snapshot_id: String,
    pub engine: String,
    pub decisions: Vec<Decision>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TargetGuard {
    pub element_id: String,
    pub fingerprint: String,
    pub backend_node_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionPlan {
    pub contract: String,
    pub snapshot_id: String,
    pub document: DocumentIdentity,
    pub operation: Operation,
    pub target: Option<TargetGuard>,
    pub value: Option<String>,
    pub confidence: f64,
    pub margin: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionResult {
    pub contract: String,
    pub status: String,
    pub operation: Operation,
    pub target_id: Option<String>,
    pub result: Option<serde_json::Value>,
    pub document: DocumentIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdapterRequest {
    Snapshot,
    Execute { plan: Box<ActionPlan> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum AdapterResponse {
    Snapshot(RawSnapshot),
    Executed(ExecutionResult),
    Error { code: String, message: String },
}

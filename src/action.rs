use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentAction {
    ClickTarget { target: String },
    TypeText { text: String },
    PressEnter,
    Done,
    Abort { reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Pending,
    Simulated,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionRecord {
    pub step: usize,
    pub action: AgentAction,
    pub status: ActionStatus,
    pub started_at: String,
    pub duration_ms: f64,
    pub error: Option<String>,
}

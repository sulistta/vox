//! Domain types for the bounded computer-use loop.
use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(transparent)]
pub struct ArtifactId(pub String);

impl ArtifactId {
    pub fn new(prefix: &str, ordinal: usize) -> Self {
        Self(format!("{prefix}_{ordinal}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    UserGoal,
    VisualInstruction,
    TextToType,
    AssistantReply,
    ClarificationQuestion,
    StructuredContext,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactSource {
    User,
    TextProvider,
    System,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextArtifact {
    pub id: ArtifactId,
    pub kind: ArtifactKind,
    pub value: String,
    pub source: ArtifactSource,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ArtifactStore {
    items: BTreeMap<ArtifactId, TextArtifact>,
    next_ordinal: usize,
}

impl ArtifactStore {
    pub fn insert(
        &mut self,
        kind: ArtifactKind,
        source: ArtifactSource,
        value: impl Into<String>,
    ) -> ArtifactId {
        let prefix = match kind {
            ArtifactKind::UserGoal => "goal",
            ArtifactKind::VisualInstruction => "visual",
            ArtifactKind::TextToType => "text",
            ArtifactKind::AssistantReply => "reply",
            ArtifactKind::ClarificationQuestion => "question",
            ArtifactKind::StructuredContext => "context",
        };
        let id = ArtifactId::new(prefix, self.next_ordinal);
        self.next_ordinal += 1;
        self.items.insert(
            id.clone(),
            TextArtifact {
                id: id.clone(),
                kind,
                value: value.into(),
                source,
            },
        );
        id
    }
    pub fn get(&self, id: &ArtifactId) -> Option<&TextArtifact> {
        self.items.get(id)
    }
    pub fn by_kind(&self, kind: ArtifactKind) -> Vec<&TextArtifact> {
        self.items
            .values()
            .filter(|item| item.kind == kind)
            .collect()
    }
    pub fn iter(&self) -> impl Iterator<Item = &TextArtifact> {
        self.items.values()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppId(pub String);
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    pub id: AppId,
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    Enter,
    Escape,
    Tab,
    Backspace,
    Space,
    Up,
    Down,
    Left,
    Right,
    A,
    C,
    V,
    X,
}
impl Key {
    pub fn choices() -> Vec<Self> {
        vec![
            Self::Enter,
            Self::Escape,
            Self::Tab,
            Self::Backspace,
            Self::Space,
            Self::Up,
            Self::Down,
            Self::Left,
            Self::Right,
            Self::A,
            Self::C,
            Self::V,
            Self::X,
        ]
    }
    pub fn choice_id(&self) -> &'static str {
        match self {
            Self::Enter => "enter",
            Self::Escape => "escape",
            Self::Tab => "tab",
            Self::Backspace => "backspace",
            Self::Space => "space",
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::A => "a",
            Self::C => "c",
            Self::V => "v",
            Self::X => "x",
        }
    }
    pub fn from_choice(value: &str) -> Option<Self> {
        Self::choices()
            .into_iter()
            .find(|candidate| candidate.choice_id() == value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}
impl ScrollDirection {
    pub fn choice_id(&self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
    pub fn all() -> [Self; 4] {
        [Self::Up, Self::Down, Self::Left, Self::Right]
    }
    pub fn from_choice(value: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|candidate| candidate.choice_id() == value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextRequestKind {
    ComposeText,
    ExtractTextToType,
    VisualInstruction,
    ReplyToUser,
    AskClarification,
    TransformText,
}
impl TextRequestKind {
    pub fn choice_id(&self) -> &'static str {
        match self {
            Self::ComposeText => "compose_text",
            Self::ExtractTextToType => "extract_text_to_type",
            Self::VisualInstruction => "visual_instruction",
            Self::ReplyToUser => "reply_to_user",
            Self::AskClarification => "ask_clarification",
            Self::TransformText => "transform_text",
        }
    }
    pub fn all() -> [Self; 6] {
        [
            Self::ComposeText,
            Self::ExtractTextToType,
            Self::VisualInstruction,
            Self::ReplyToUser,
            Self::AskClarification,
            Self::TransformText,
        ]
    }
    pub fn from_choice(value: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|candidate| candidate.choice_id() == value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    LaunchApp,
    VisualAction,
    TypeArtifact,
    PressKey,
    Hotkey,
    Scroll,
    RequestText,
    AskUser,
    ReplyToUser,
    Wait,
    Done,
    Abort,
}
impl Capability {
    pub fn choice_id(&self) -> &'static str {
        match self {
            Self::LaunchApp => "launch_app",
            Self::VisualAction => "visual_action",
            Self::TypeArtifact => "type_artifact",
            Self::PressKey => "press_key",
            Self::Hotkey => "hotkey",
            Self::Scroll => "scroll",
            Self::RequestText => "request_text",
            Self::AskUser => "ask_user",
            Self::ReplyToUser => "reply_to_user",
            Self::Wait => "wait",
            Self::Done => "done",
            Self::Abort => "abort",
        }
    }
    pub fn all() -> [Self; 12] {
        [
            Self::LaunchApp,
            Self::VisualAction,
            Self::TypeArtifact,
            Self::PressKey,
            Self::Hotkey,
            Self::Scroll,
            Self::RequestText,
            Self::AskUser,
            Self::ReplyToUser,
            Self::Wait,
            Self::Done,
            Self::Abort,
        ]
    }
    pub fn from_choice(value: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|candidate| candidate.choice_id() == value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentAction {
    LaunchApp { app_id: AppId },
    VisualAction { instruction_artifact: ArtifactId },
    TypeArtifact { artifact: ArtifactId },
    PressKey { key: Key },
    Hotkey { keys: Vec<Key> },
    Scroll { direction: ScrollDirection },
    RequestText { kind: TextRequestKind },
    AskUser { artifact: ArtifactId },
    ReplyToUser { artifact: ArtifactId },
    Wait { duration_ms: u64 },
    Done,
    Abort,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputResult {
    Delivered,
    Failed,
    Simulated,
    NotApplicable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationResult {
    ScreenChanged,
    NoVisibleChange,
    Unknown,
    NotApplicable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Planned,
    Executed,
    InputFailed,
    NoVisibleChange,
    Failed,
    Simulated,
    AwaitingUser,
    AwaitingConfirmation,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionRecord {
    pub step: usize,
    pub selected_action: AgentAction,
    pub status: ActionStatus,
    pub input_result: InputResult,
    pub verification: VerificationResult,
    pub started_at_ms: u128,
    pub duration_ms: f64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}
impl RiskLevel {
    pub fn choice_id(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
    pub fn from_choice(value: &str) -> Option<Self> {
        match value {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub action: AgentAction,
    pub input_result: InputResult,
    pub verification: VerificationResult,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityAvailability {
    pub vision: bool,
    pub text_provider: bool,
    pub desktop_input: bool,
    pub app_catalog: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DesktopState {
    pub live_capture: bool,
    pub execute_enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentState {
    pub run_id: String,
    pub goal: ArtifactId,
    pub artifacts: ArtifactStore,
    pub recent_history: Vec<ActionRecord>,
    pub last_execution: Option<ExecutionRecord>,
    pub available_capabilities: CapabilityAvailability,
    pub desktop_state: DesktopState,
    pub installed_apps: Vec<AppInfo>,
}
impl AgentState {
    pub fn new(
        goal: impl Into<String>,
        availability: CapabilityAvailability,
        desktop_state: DesktopState,
        apps: Vec<AppInfo>,
    ) -> Self {
        let mut artifacts = ArtifactStore::default();
        let goal = artifacts.insert(ArtifactKind::UserGoal, ArtifactSource::User, goal);
        Self {
            run_id: Uuid::new_v4().to_string(),
            goal,
            artifacts,
            recent_history: Vec::new(),
            last_execution: None,
            available_capabilities: availability,
            desktop_state,
            installed_apps: apps,
        }
    }
    pub fn push_record(&mut self, record: ActionRecord) {
        self.last_execution = Some(ExecutionRecord {
            action: record.selected_action.clone(),
            input_result: record.input_result.clone(),
            verification: record.verification.clone(),
        });
        self.recent_history.push(record);
        const HISTORY_LIMIT: usize = 8;
        if self.recent_history.len() > HISTORY_LIMIT {
            self.recent_history
                .drain(..self.recent_history.len() - HISTORY_LIMIT);
        }
    }
}
pub fn fixed_wait_durations() -> [Duration; 4] {
    [
        Duration::from_millis(250),
        Duration::from_millis(500),
        Duration::from_millis(1000),
        Duration::from_millis(2000),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artifact_store_inserts_filters_and_retrieves() {
        let mut store = ArtifactStore::default();
        let goal = store.insert(ArtifactKind::UserGoal, ArtifactSource::User, "do something");
        let text = store.insert(
            ArtifactKind::TextToType,
            ArtifactSource::TextProvider,
            "hello",
        );
        assert_eq!(store.get(&goal).unwrap().value, "do something");
        assert_eq!(store.by_kind(ArtifactKind::TextToType)[0].id, text);
        assert_eq!(
            store.by_kind(ArtifactKind::TextToType)[0].source,
            ArtifactSource::TextProvider
        );
    }
}

//! Domain types for the bounded, evidence-driven computer-use loop.
use std::collections::BTreeMap;

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

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(transparent)]
pub struct GuidanceArtifactId(pub String);
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(transparent)]
pub struct GuidanceHintId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    UserGoal,
    UserReply,
    VisualInstruction,
    TextToType,
    AssistantReply,
    ClarificationQuestion,
    ActionPreview,
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
            ArtifactKind::UserReply => "user",
            ArtifactKind::VisualInstruction => "visual",
            ArtifactKind::TextToType => "text",
            ArtifactKind::AssistantReply => "reply",
            ArtifactKind::ClarificationQuestion => "question",
            ArtifactKind::ActionPreview => "preview",
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
    Up,
    Down,
    Left,
    Right,
    Control,
    A,
    C,
    V,
    X,
}
impl Key {
    pub fn choice_id(&self) -> &'static str {
        match self {
            Self::Enter => "enter",
            Self::Escape => "escape",
            Self::Tab => "tab",
            Self::Backspace => "backspace",
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Control => "control",
            Self::A => "a",
            Self::C => "c",
            Self::V => "v",
            Self::X => "x",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shortcut {
    SelectAll,
    Copy,
    Cut,
    Paste,
}
impl Shortcut {
    pub fn choice_id(&self) -> &'static str {
        match self {
            Self::SelectAll => "select_all",
            Self::Copy => "copy",
            Self::Cut => "cut",
            Self::Paste => "paste",
        }
    }
    pub fn keys(&self) -> Vec<Key> {
        let key = match self {
            Self::SelectAll => Key::A,
            Self::Copy => Key::C,
            Self::Cut => Key::X,
            Self::Paste => Key::V,
        };
        vec![Key::Control, key]
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
    TaskGuidance,
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
            Self::TaskGuidance => "task_guidance",
        }
    }
    pub fn output_kind(&self) -> Option<GuidanceArtifactKind> {
        match self {
            Self::ComposeText | Self::ExtractTextToType | Self::TransformText => {
                Some(GuidanceArtifactKind::TextToType)
            }
            Self::VisualInstruction => Some(GuidanceArtifactKind::VisualInstruction),
            Self::ReplyToUser => Some(GuidanceArtifactKind::AssistantReply),
            Self::AskClarification => Some(GuidanceArtifactKind::ClarificationQuestion),
            Self::TaskGuidance => None,
        }
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
    RequestPlan,
    RequestReplan,
    AskUser,
    ReplyToUser,
    Wait,
    Commit,
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
            Self::RequestPlan => "request_plan",
            Self::RequestReplan => "request_replan",
            Self::AskUser => "ask_user",
            Self::ReplyToUser => "reply_to_user",
            Self::Wait => "wait",
            Self::Commit => "commit",
            Self::Done => "done",
            Self::Abort => "abort",
        }
    }
    pub fn from_choice(value: &str) -> Option<Self> {
        [
            Self::LaunchApp,
            Self::VisualAction,
            Self::TypeArtifact,
            Self::PressKey,
            Self::Hotkey,
            Self::Scroll,
            Self::RequestText,
            Self::RequestPlan,
            Self::RequestReplan,
            Self::AskUser,
            Self::ReplyToUser,
            Self::Wait,
            Self::Commit,
            Self::Done,
            Self::Abort,
        ]
        .into_iter()
        .find(|item| item.choice_id() == value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuidanceArtifactKind {
    VisualInstruction,
    TextToType,
    AssistantReply,
    ClarificationQuestion,
    ActionPreview,
}
impl GuidanceArtifactKind {
    pub fn artifact_kind(&self) -> ArtifactKind {
        match self {
            Self::VisualInstruction => ArtifactKind::VisualInstruction,
            Self::TextToType => ArtifactKind::TextToType,
            Self::AssistantReply => ArtifactKind::AssistantReply,
            Self::ClarificationQuestion => ArtifactKind::ClarificationQuestion,
            Self::ActionPreview => ArtifactKind::ActionPreview,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GuidanceArtifactContent {
    Literal {
        value: String,
    },
    Generate {
        kind: TextRequestKind,
        purpose: String,
        #[serde(default)]
        requirements: Vec<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidanceArtifactSpec {
    pub id: GuidanceArtifactId,
    pub kind: GuidanceArtifactKind,
    pub content: GuidanceArtifactContent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommitOperation {
    VisualAction { artifact: GuidanceArtifactId },
    PressKey { key: Key },
    Hotkey { shortcut: Shortcut },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionHintKind {
    LaunchApp,
    VisualAction {
        artifact: GuidanceArtifactId,
    },
    TypeArtifact {
        artifact: GuidanceArtifactId,
    },
    PressKey {
        key: Key,
    },
    Hotkey {
        shortcut: Shortcut,
    },
    Scroll {
        direction: ScrollDirection,
    },
    AskUser {
        artifact: GuidanceArtifactId,
    },
    ReplyToUser {
        artifact: GuidanceArtifactId,
    },
    Wait {
        duration: WaitDuration,
    },
    Commit {
        preview: GuidanceArtifactId,
        operation: CommitOperation,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionHint {
    pub id: GuidanceHintId,
    pub action: ActionHintKind,
    #[serde(default)]
    pub terminal: bool,
}

/// Non-authoritative context. It has no cursor, order, dependencies, or execution semantics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskGuidance {
    pub objective: String,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub artifacts: Vec<GuidanceArtifactSpec>,
    pub hints: Vec<ActionHint>,
}
impl TaskGuidance {
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: Self = serde_json::from_str(text)
            .map_err(|error| format!("TaskGuidance must be JSON: {error}"))?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), String> {
        valid_text(&self.objective, 512, "objective")?;
        if self.constraints.len() > 8
            || self.artifacts.len() > 16
            || self.hints.is_empty()
            || self.hints.len() > 16
        {
            return Err("TaskGuidance exceeds bounded limits or has no hints".into());
        }
        let mut artifacts = BTreeMap::new();
        for spec in &self.artifacts {
            valid_text(&spec.id.0, 64, "artifact id")?;
            if artifacts
                .insert(spec.id.clone(), spec.kind.clone())
                .is_some()
            {
                return Err("TaskGuidance has duplicate artifact ids".into());
            }
            match &spec.content {
                GuidanceArtifactContent::Literal { value } => {
                    valid_text(value, 4096, "artifact value")?
                }
                GuidanceArtifactContent::Generate {
                    kind,
                    purpose,
                    requirements,
                } => {
                    if kind.output_kind() != Some(spec.kind.clone()) {
                        return Err(
                            "generated artifact kind is incompatible with its request kind".into(),
                        );
                    }
                    valid_text(purpose, 1024, "generation purpose")?;
                    if requirements.len() > 8 {
                        return Err("too many generation requirements".into());
                    }
                    for requirement in requirements {
                        valid_text(requirement, 512, "generation requirement")?;
                    }
                }
            }
        }
        for constraint in &self.constraints {
            valid_text(constraint, 512, "constraint")?;
        }
        let mut hints = BTreeMap::new();
        for hint in &self.hints {
            valid_text(&hint.id.0, 64, "hint id")?;
            if hints.insert(hint.id.clone(), ()).is_some() {
                return Err("TaskGuidance has duplicate hint ids".into());
            }
            let lookup = |id: &GuidanceArtifactId, expected: GuidanceArtifactKind| {
                (artifacts.get(id) == Some(&expected))
                    .then_some(())
                    .ok_or_else(|| {
                        format!("hint references invalid {expected:?} artifact {}", id.0)
                    })
            };
            match &hint.action {
                ActionHintKind::LaunchApp
                | ActionHintKind::PressKey { .. }
                | ActionHintKind::Hotkey { .. }
                | ActionHintKind::Scroll { .. }
                | ActionHintKind::Wait { .. } => {}
                ActionHintKind::VisualAction { artifact } => {
                    lookup(artifact, GuidanceArtifactKind::VisualInstruction)?
                }
                ActionHintKind::TypeArtifact { artifact } => {
                    lookup(artifact, GuidanceArtifactKind::TextToType)?
                }
                ActionHintKind::AskUser { artifact } => {
                    lookup(artifact, GuidanceArtifactKind::ClarificationQuestion)?
                }
                ActionHintKind::ReplyToUser { artifact } => {
                    lookup(artifact, GuidanceArtifactKind::AssistantReply)?
                }
                ActionHintKind::Commit { preview, operation } => {
                    lookup(preview, GuidanceArtifactKind::ActionPreview)?;
                    if let CommitOperation::VisualAction { artifact } = operation {
                        lookup(artifact, GuidanceArtifactKind::VisualInstruction)?;
                    }
                }
            }
        }
        Ok(())
    }
}
fn valid_text(value: &str, max: usize, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        Err(format!("{field} must be non-empty and at most {max} bytes"))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitDuration {
    Short,
    Medium,
    Long,
}
impl WaitDuration {
    pub fn milliseconds(&self) -> u64 {
        match self {
            Self::Short => 300,
            Self::Medium => 1_000,
            Self::Long => 2_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitOperationResolved {
    VisualAction { instruction_artifact: ArtifactId },
    PressKey { key: Key },
    Hotkey { shortcut: Shortcut },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentAction {
    LaunchApp {
        app_id: AppId,
    },
    VisualAction {
        instruction_artifact: ArtifactId,
    },
    TypeArtifact {
        artifact: ArtifactId,
    },
    PressKey {
        key: Key,
    },
    Hotkey {
        shortcut: Shortcut,
    },
    Scroll {
        direction: ScrollDirection,
    },
    RequestText {
        artifact: GuidanceArtifactId,
        kind: TextRequestKind,
    },
    RequestPlan,
    RequestReplan,
    AskUser {
        artifact: ArtifactId,
    },
    ReplyToUser {
        artifact: ArtifactId,
    },
    Wait {
        duration: WaitDuration,
    },
    Commit {
        preview_artifact: ArtifactId,
        operation: CommitOperationResolved,
    },
    Done,
    Abort,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionSource {
    FastPath,
    Guidance(GuidanceHintId),
    System,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedAction {
    pub action: AgentAction,
    pub source: ActionSource,
    pub hint_id: Option<GuidanceHintId>,
    pub terminal: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HintStatus {
    Available,
    Attempted { step: usize },
    Succeeded { step: usize },
    Failed { step: usize },
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
    GroundingNotFound,
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
    pub hint_id: Option<GuidanceHintId>,
    pub status: ActionStatus,
    pub input_result: InputResult,
    pub verification: VerificationResult,
    pub started_at_ms: u128,
    pub duration_ms: f64,
    pub error: Option<String>,
    pub state_revision: u64,
    pub observation_revision: u64,
    pub guidance_revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplanReason {
    GroundingNotFound { hint_id: Option<GuidanceHintId> },
    InputFailed { hint_id: Option<GuidanceHintId> },
    VerificationFailed { hint_id: Option<GuidanceHintId> },
    GuidanceExhausted,
    LoopDetected,
    UserProvidedInformation,
    ProviderBecameUnavailable,
    OtherExplicitFailure,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionResult {
    OperationallyCompleteUnverified,
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
    pub text_input: bool,
    pub app_catalog: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DesktopState {
    pub live_capture: bool,
    pub execute_enabled: bool,
    pub last_grounded_frame: Option<FrameObservation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameObservation {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub logical_width: u32,
    pub logical_height: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuidanceRuntime {
    pub guidance: TaskGuidance,
    pub artifacts: BTreeMap<GuidanceArtifactId, Option<ArtifactId>>,
    pub hints: BTreeMap<GuidanceHintId, HintStatus>,
}
impl GuidanceRuntime {
    fn new(guidance: TaskGuidance, store: &mut ArtifactStore) -> Self {
        let artifacts = guidance
            .artifacts
            .iter()
            .map(|spec| {
                let item = match &spec.content {
                    GuidanceArtifactContent::Literal { value } => Some(store.insert(
                        spec.kind.artifact_kind(),
                        ArtifactSource::TextProvider,
                        value.clone(),
                    )),
                    GuidanceArtifactContent::Generate { .. } => None,
                };
                (spec.id.clone(), item)
            })
            .collect();
        let hints = guidance
            .hints
            .iter()
            .map(|hint| (hint.id.clone(), HintStatus::Available))
            .collect();
        Self {
            guidance,
            artifacts,
            hints,
        }
    }
    pub fn artifact(&self, id: &GuidanceArtifactId) -> Option<&ArtifactId> {
        self.artifacts.get(id).and_then(Option::as_ref)
    }
    pub fn pending(&self) -> impl Iterator<Item = &GuidanceArtifactSpec> {
        self.guidance
            .artifacts
            .iter()
            .filter(|spec| self.artifacts.get(&spec.id).is_some_and(Option::is_none))
    }
    pub fn hint(&self, id: &GuidanceHintId) -> Option<&ActionHint> {
        self.guidance.hints.iter().find(|hint| &hint.id == id)
    }
    pub fn available_hints(&self) -> impl Iterator<Item = &ActionHint> {
        self.guidance
            .hints
            .iter()
            .filter(|hint| self.hints.get(&hint.id) == Some(&HintStatus::Available))
    }
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
    pub guidance: Option<GuidanceRuntime>,
    pub replan_reason: Option<ReplanReason>,
    pub completion: Option<CompletionResult>,
    pub state_revision: u64,
    pub observation_revision: u64,
    pub guidance_revision: u64,
    pub terminal_evidence: bool,
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
            recent_history: vec![],
            last_execution: None,
            available_capabilities: availability,
            desktop_state,
            installed_apps: apps,
            guidance: None,
            replan_reason: None,
            completion: None,
            state_revision: 0,
            observation_revision: 0,
            guidance_revision: 0,
            terminal_evidence: false,
        }
    }
    pub fn set_guidance(&mut self, guidance: TaskGuidance) {
        self.guidance = Some(GuidanceRuntime::new(guidance, &mut self.artifacts));
        self.replan_reason = None;
        self.terminal_evidence = false;
        self.guidance_revision += 1;
        self.state_revision += 1;
    }
    pub fn materialize_generated(
        &mut self,
        alias: &GuidanceArtifactId,
        value: String,
    ) -> Result<ArtifactId, String> {
        let runtime = self.guidance.as_mut().ok_or("no active guidance")?;
        let spec = runtime
            .guidance
            .artifacts
            .iter()
            .find(|spec| &spec.id == alias)
            .ok_or("unknown guidance artifact")?;
        if !matches!(spec.content, GuidanceArtifactContent::Generate { .. })
            || runtime.artifact(alias).is_some()
        {
            return Err("guidance artifact is not pending generation".into());
        }
        let id = self.artifacts.insert(
            spec.kind.artifact_kind(),
            ArtifactSource::TextProvider,
            value,
        );
        runtime.artifacts.insert(alias.clone(), Some(id.clone()));
        self.state_revision += 1;
        Ok(id)
    }
    pub fn insert_artifact(
        &mut self,
        kind: ArtifactKind,
        source: ArtifactSource,
        value: impl Into<String>,
    ) -> ArtifactId {
        let id = self.artifacts.insert(kind, source, value);
        self.state_revision += 1;
        id
    }
    pub fn mark_hint(&mut self, id: &GuidanceHintId, status: HintStatus) {
        if let Some(runtime) = self.guidance.as_mut() {
            runtime.hints.insert(id.clone(), status);
        }
    }
    pub fn set_replan(&mut self, reason: ReplanReason) {
        if self.replan_reason.is_none() {
            self.replan_reason = Some(reason);
            self.state_revision += 1;
        }
    }
    pub fn push_record(&mut self, record: ActionRecord) {
        let visual_action = matches!(
            record.selected_action,
            AgentAction::VisualAction { .. }
                | AgentAction::Commit {
                    operation: CommitOperationResolved::VisualAction { .. },
                    ..
                }
        );
        if let Some(id) = &record.hint_id {
            let success = matches!(record.status, ActionStatus::Executed)
                && matches!(
                    record.input_result,
                    InputResult::Delivered | InputResult::NotApplicable
                )
                && !matches!(record.verification, VerificationResult::GroundingNotFound)
                && !(visual_action
                    && matches!(record.verification, VerificationResult::NoVisibleChange));
            self.mark_hint(
                id,
                if success {
                    HintStatus::Succeeded { step: record.step }
                } else {
                    HintStatus::Failed { step: record.step }
                },
            );
            if success
                && self
                    .guidance
                    .as_ref()
                    .and_then(|runtime| runtime.hint(id))
                    .is_some_and(|hint| hint.terminal)
            {
                self.terminal_evidence = true;
            }
        }
        if self.replan_reason.is_none() {
            match (&record.input_result, &record.verification, &record.status) {
                (_, VerificationResult::GroundingNotFound, _) => {
                    self.set_replan(ReplanReason::GroundingNotFound {
                        hint_id: record.hint_id.clone(),
                    })
                }
                (InputResult::Failed, _, _) | (_, _, ActionStatus::InputFailed) => {
                    self.set_replan(ReplanReason::InputFailed {
                        hint_id: record.hint_id.clone(),
                    })
                }
                (_, VerificationResult::NoVisibleChange, _) if visual_action => {
                    self.set_replan(ReplanReason::VerificationFailed {
                        hint_id: record.hint_id.clone(),
                    })
                }
                (_, _, ActionStatus::NoVisibleChange) => {
                    self.set_replan(ReplanReason::VerificationFailed {
                        hint_id: record.hint_id.clone(),
                    })
                }
                (_, _, ActionStatus::Failed) => self.set_replan(ReplanReason::OtherExplicitFailure),
                _ => {}
            }
        }
        self.last_execution = Some(ExecutionRecord {
            action: record.selected_action.clone(),
            input_result: record.input_result.clone(),
            verification: record.verification.clone(),
        });
        self.recent_history.push(record);
        if self.recent_history.len() > 8 {
            self.recent_history.drain(..self.recent_history.len() - 8);
        }
        if self.replan_reason.is_none() && repeated_physical_pattern(&self.recent_history) {
            self.set_replan(ReplanReason::LoopDetected);
        }
    }
    pub fn note_visual_observation(&mut self) {
        self.observation_revision += 1;
        self.state_revision += 1;
    }
    pub fn mark_completed(&mut self) {
        self.completion = Some(CompletionResult::OperationallyCompleteUnverified);
        self.state_revision += 1;
    }
}

/// Artifact IDs are intentionally ignored so separate hints that repeat the
/// same physical operation cannot evade the loop guard. Three repetitions are
/// required, and a new state/observation/guidance revision breaks the pattern.
fn repeated_physical_pattern(records: &[ActionRecord]) -> bool {
    for width in 1..=4 {
        let needed = width * 3;
        if records.len() < needed {
            continue;
        }
        let tail = &records[records.len() - needed..];
        if !tail.iter().all(|record| {
            record.state_revision == tail[0].state_revision
                && record.observation_revision == tail[0].observation_revision
                && record.guidance_revision == tail[0].guidance_revision
        }) {
            continue;
        }
        let first: Vec<_> = tail[..width].iter().map(physical_fingerprint).collect();
        if tail[width..].chunks(width).all(|chunk| {
            chunk
                .iter()
                .map(physical_fingerprint)
                .eq(first.iter().cloned())
        }) {
            return true;
        }
    }
    false
}

fn physical_fingerprint(record: &ActionRecord) -> String {
    match &record.selected_action {
        AgentAction::LaunchApp { app_id } => format!("launch:{}", app_id.0),
        AgentAction::VisualAction { .. } => "visual".into(),
        AgentAction::TypeArtifact { .. } => "type".into(),
        AgentAction::PressKey { key } => format!("key:{}", key.choice_id()),
        AgentAction::Hotkey { shortcut } => format!("hotkey:{}", shortcut.choice_id()),
        AgentAction::Scroll { direction } => format!("scroll:{}", direction.choice_id()),
        AgentAction::RequestText { kind, .. } => format!("text:{}", kind.choice_id()),
        AgentAction::RequestPlan => "request_plan".into(),
        AgentAction::RequestReplan => "request_replan".into(),
        AgentAction::AskUser { .. } => "ask_user".into(),
        AgentAction::ReplyToUser { .. } => "reply_user".into(),
        AgentAction::Wait { duration } => format!("wait:{}", duration.milliseconds()),
        AgentAction::Commit { .. } => "commit".into(),
        AgentAction::Done => "done".into(),
        AgentAction::Abort => "abort".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guidance_materializes_literals_and_leaves_generated_artifacts_pending() {
        let guidance = TaskGuidance::parse(r#"{"objective":"test","artifacts":[{"id":"target","kind":"visual_instruction","content":{"type":"literal","value":"Click the button"}},{"id":"message","kind":"text_to_type","content":{"type":"generate","kind":"compose_text","purpose":"Write greeting"}}],"hints":[{"id":"click","action":{"type":"visual_action","artifact":"target"}},{"id":"type","action":{"type":"type_artifact","artifact":"message"},"terminal":true}]}"#).unwrap();
        let mut state = AgentState::new(
            "goal",
            CapabilityAvailability {
                vision: true,
                text_provider: true,
                desktop_input: true,
                text_input: true,
                app_catalog: true,
            },
            DesktopState {
                live_capture: true,
                execute_enabled: false,
                last_grounded_frame: None,
            },
            vec![],
        );
        state.set_guidance(guidance);
        let runtime = state.guidance.as_ref().unwrap();
        assert!(
            runtime
                .artifact(&GuidanceArtifactId("target".into()))
                .is_some()
        );
        assert!(
            runtime
                .artifact(&GuidanceArtifactId("message".into()))
                .is_none()
        );
    }
    #[test]
    fn guidance_rejects_invalid_references() {
        assert!(TaskGuidance::parse(r#"{"objective":"test","artifacts":[],"hints":[{"id":"click","action":{"type":"visual_action","artifact":"missing"}}]}"#).is_err());
    }

    #[test]
    fn guidance_rejects_duplicate_ids_and_incompatible_generated_kind() {
        assert!(TaskGuidance::parse(r#"{"objective":"test","artifacts":[{"id":"same","kind":"visual_instruction","content":{"type":"literal","value":"Click"}},{"id":"same","kind":"visual_instruction","content":{"type":"literal","value":"Click again"}}],"hints":[{"id":"one","action":{"type":"visual_action","artifact":"same"}}]}"#).is_err());
        assert!(TaskGuidance::parse(r#"{"objective":"test","artifacts":[{"id":"message","kind":"text_to_type","content":{"type":"generate","kind":"visual_instruction","purpose":"Wrong kind"}}],"hints":[{"id":"type","action":{"type":"type_artifact","artifact":"message"}}]}"#).is_err());
    }

    #[test]
    fn nonvisual_no_change_is_not_a_replan_reason() {
        let mut state = AgentState::new(
            "goal",
            CapabilityAvailability {
                vision: true,
                text_provider: true,
                desktop_input: true,
                text_input: true,
                app_catalog: true,
            },
            DesktopState {
                live_capture: true,
                execute_enabled: true,
                last_grounded_frame: None,
            },
            vec![],
        );
        state.push_record(ActionRecord {
            step: 1,
            selected_action: AgentAction::PressKey { key: Key::Enter },
            hint_id: None,
            status: ActionStatus::Executed,
            input_result: InputResult::Delivered,
            verification: VerificationResult::NoVisibleChange,
            started_at_ms: 0,
            duration_ms: 0.0,
            error: None,
            state_revision: 0,
            observation_revision: 0,
            guidance_revision: 0,
        });
        assert_eq!(state.replan_reason, None);
    }

    #[test]
    fn loop_detector_catches_alternating_physical_keys() {
        let make = |step, key| ActionRecord {
            step,
            selected_action: AgentAction::PressKey { key },
            hint_id: None,
            status: ActionStatus::Executed,
            input_result: InputResult::Delivered,
            verification: VerificationResult::Unknown,
            started_at_ms: 0,
            duration_ms: 0.0,
            error: None,
            state_revision: 4,
            observation_revision: 0,
            guidance_revision: 1,
        };
        let records = vec![
            make(1, Key::Enter),
            make(2, Key::Escape),
            make(3, Key::Enter),
            make(4, Key::Escape),
            make(5, Key::Enter),
            make(6, Key::Escape),
        ];
        assert!(repeated_physical_pattern(&records));
    }

    #[test]
    fn guidance_rejects_more_than_eight_constraints() {
        let constraints = (0..9)
            .map(|index| format!("constraint {index}"))
            .collect::<Vec<_>>();
        let guidance = TaskGuidance {
            objective: "test".into(),
            constraints,
            artifacts: vec![],
            hints: vec![ActionHint {
                id: GuidanceHintId("wait".into()),
                action: ActionHintKind::Wait {
                    duration: WaitDuration::Short,
                },
                terminal: false,
            }],
        };
        assert!(guidance.validate().is_err());
    }

    #[test]
    fn completion_is_explicitly_operational_not_semantic() {
        let mut state = AgentState::new(
            "goal",
            CapabilityAvailability {
                vision: false,
                text_provider: false,
                desktop_input: false,
                text_input: false,
                app_catalog: false,
            },
            DesktopState {
                live_capture: false,
                execute_enabled: false,
                last_grounded_frame: None,
            },
            vec![],
        );
        state.mark_completed();
        assert_eq!(
            state.completion,
            Some(CompletionResult::OperationallyCompleteUnverified)
        );
    }
}

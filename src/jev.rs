use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::action::{
    AgentAction, AgentState, ArtifactId, ArtifactKind, Capability, Key, RiskLevel, ScrollDirection,
    TextRequestKind, fixed_wait_durations,
};

const NONE: &str = "__none__";

#[derive(Debug, Error)]
pub enum DecisionError {
    #[error("Jev is not configured")]
    NotConfigured,
    #[error("TypeSafe request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid Jev response: {0}")]
    InvalidResponse(String),
    #[error("decision could not be resolved: {0}")]
    Resolution(String),
}

#[derive(Clone, Debug)]
pub struct JevConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
}
#[derive(Clone, Debug)]
pub struct ChoiceAnswer {
    pub choice: String,
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: f64,
}
#[derive(Clone, Debug)]
pub struct JevDecision {
    pub action: ChoiceAnswer,
    pub application: ChoiceAnswer,
    pub visual_artifact: ChoiceAnswer,
    pub type_artifact: ChoiceAnswer,
    pub user_message_artifact: ChoiceAnswer,
    pub key: ChoiceAnswer,
    pub scroll_direction: ChoiceAnswer,
    pub wait_duration: ChoiceAnswer,
    pub text_request_kind: ChoiceAnswer,
    pub risk_level: ChoiceAnswer,
    pub request_id: Option<String>,
    pub latency_ms: f64,
    pub model: Option<String>,
}

#[async_trait]
pub trait DecisionProvider: Send + Sync {
    async fn decide(&self, state: &AgentState) -> Result<JevDecision, DecisionError>;
    async fn test_connection(&self) -> Result<(), DecisionError>;
}
#[derive(Clone)]
pub struct JevClient {
    client: Client,
    config: JevConfig,
}

#[derive(Serialize)]
struct JevRequest {
    model: String,
    state: Value,
    questions: BTreeMap<String, ChoiceQuestion>,
}
#[derive(Serialize)]
struct ChoiceQuestion {
    #[serde(rename = "type")]
    kind: &'static str,
    instructions: String,
    criteria: BTreeMap<String, Value>,
}
#[derive(Deserialize)]
struct JevResponse {
    answers: BTreeMap<String, WireChoiceAnswer>,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
}
#[derive(Deserialize)]
struct WireChoiceAnswer {
    choice: String,
    #[serde(default)]
    probabilities: BTreeMap<String, f64>,
    confidence: f64,
}

impl JevClient {
    pub fn new(client: Client, config: JevConfig) -> Self {
        Self { client, config }
    }
    fn endpoint(&self) -> String {
        format!(
            "{}/v1/systemone",
            self.config.base_url.trim_end_matches('/')
        )
    }
}

#[async_trait]
impl DecisionProvider for JevClient {
    async fn decide(&self, state: &AgentState) -> Result<JevDecision, DecisionError> {
        let api_key = self
            .config
            .api_key
            .as_ref()
            .filter(|key| !key.is_empty())
            .ok_or(DecisionError::NotConfigured)?;
        let request = build_request(state, &self.config.model);
        let started = Instant::now();
        let parsed: JevResponse = self
            .client
            .post(self.endpoint())
            .bearer_auth(api_key)
            .json(&request)
            .timeout(self.config.timeout)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(|error| DecisionError::InvalidResponse(error.to_string()))?;
        let answer = |name: &str| -> Result<ChoiceAnswer, DecisionError> {
            let wire = parsed
                .answers
                .get(name)
                .ok_or_else(|| DecisionError::InvalidResponse(format!("missing answers.{name}")))?;
            if !(0.0..=1.0).contains(&wire.confidence) || !wire.confidence.is_finite() {
                return Err(DecisionError::InvalidResponse(format!(
                    "answers.{name}.confidence is invalid"
                )));
            }
            Ok(ChoiceAnswer {
                choice: wire.choice.clone(),
                probabilities: wire.probabilities.clone(),
                confidence: wire.confidence,
            })
        };
        Ok(JevDecision {
            action: answer("action")?,
            application: answer("application")?,
            visual_artifact: answer("visual_artifact")?,
            type_artifact: answer("type_artifact")?,
            user_message_artifact: answer("user_message_artifact")?,
            key: answer("key")?,
            scroll_direction: answer("scroll_direction")?,
            wait_duration: answer("wait_duration")?,
            text_request_kind: answer("text_request_kind")?,
            risk_level: answer("risk_level")?,
            request_id: parsed.request_id,
            latency_ms: started.elapsed().as_secs_f64() * 1000.0,
            model: parsed.model,
        })
    }
    async fn test_connection(&self) -> Result<(), DecisionError> {
        let state = AgentState::new(
            "connection test",
            crate::action::CapabilityAvailability {
                vision: false,
                text_provider: false,
                desktop_input: false,
                app_catalog: false,
            },
            crate::action::DesktopState {
                live_capture: false,
                execute_enabled: false,
            },
            vec![],
        );
        self.decide(&state).await.map(|_| ())
    }
}

fn build_request(state: &AgentState, model: &str) -> JevRequest {
    let goal = state
        .artifacts
        .get(&state.goal)
        .map(|artifact| artifact.value.clone())
        .unwrap_or_default();
    let artifacts: Vec<Value> = state.artifacts.iter().take(16).map(|artifact| serde_json::json!({ "id": artifact.id, "kind": artifact.kind, "text": artifact.value, "source": artifact.source })).collect();
    let history: Vec<Value> = state.recent_history.iter().map(|record| serde_json::json!({ "action": record.selected_action, "status": record.status, "input": record.input_result, "verification": record.verification })).collect();
    let compact_state = serde_json::json!({ "goal": { "artifact_id": state.goal, "text": goal }, "artifacts": artifacts, "recent_history": history, "last_execution": state.last_execution, "availability": state.available_capabilities, "desktop": state.desktop_state, "installed_apps": state.installed_apps });
    JevRequest {
        model: model.into(),
        state: compact_state,
        questions: build_questions(state),
    }
}

fn criteria(entries: impl IntoIterator<Item = (String, String)>) -> BTreeMap<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key, Value::String(value)))
        .collect()
}
fn none_criteria() -> (String, String) {
    (NONE.into(), "No currently valid value applies.".into())
}
fn choice(
    instructions: &str,
    entries: impl IntoIterator<Item = (String, String)>,
) -> ChoiceQuestion {
    ChoiceQuestion {
        kind: "choice",
        instructions: instructions.into(),
        criteria: criteria(entries),
    }
}

fn build_questions(state: &AgentState) -> BTreeMap<String, ChoiceQuestion> {
    let mut questions = BTreeMap::new();
    let has_visual = state.artifacts.iter().any(|artifact| {
        matches!(
            artifact.kind,
            ArtifactKind::UserGoal | ArtifactKind::VisualInstruction
        )
    });
    let has_text = !state.artifacts.by_kind(ArtifactKind::TextToType).is_empty();
    let has_question = !state
        .artifacts
        .by_kind(ArtifactKind::ClarificationQuestion)
        .is_empty();
    let has_reply = !state
        .artifacts
        .by_kind(ArtifactKind::AssistantReply)
        .is_empty();
    let mut actions = vec![
        (
            Capability::PressKey.choice_id().into(),
            "Send one supported known keyboard key.".into(),
        ),
        (
            Capability::Hotkey.choice_id().into(),
            "Send a supported keyboard shortcut.".into(),
        ),
        (
            Capability::Scroll.choice_id().into(),
            "Reveal additional visible content using one scroll operation.".into(),
        ),
        (
            Capability::Wait.choice_id().into(),
            "Wait briefly for expected UI change without input.".into(),
        ),
        (
            Capability::Done.choice_id().into(),
            "Use only when the goal has sufficient evidence of completion.".into(),
        ),
        (
            Capability::Abort.choice_id().into(),
            "Use when it is impossible or unsafe to continue.".into(),
        ),
    ];
    if !state.installed_apps.is_empty() {
        actions.push((
            Capability::LaunchApp.choice_id().into(),
            "Launch an installed application when that is the next useful operation.".into(),
        ));
    }
    if has_visual && state.available_capabilities.vision {
        actions.push((
            Capability::VisualAction.choice_id().into(),
            "Locate and interact with a visible target using an existing concise visual instruction.".into(),
        ));
    }
    if has_text {
        actions.push((
            Capability::TypeArtifact.choice_id().into(),
            "Enter an existing text artifact into the focused UI.".into(),
        ));
    }
    if state.available_capabilities.text_provider {
        actions.push((
            Capability::RequestText.choice_id().into(),
            "Request new free-form language only when it does not already exist as an artifact."
                .into(),
        ));
    }
    if has_question {
        actions.push((
            Capability::AskUser.choice_id().into(),
            "Present an existing clarification-question artifact and pause.".into(),
        ));
    }
    if has_reply {
        actions.push((
            Capability::ReplyToUser.choice_id().into(),
            "Present an existing assistant-reply artifact.".into(),
        ));
    }
    questions.insert("action".into(), choice("Choose exactly one next bounded capability that is available in the current state. Rust owns control flow; choose only the next operation, never a plan.", actions));
    let mut apps: Vec<(String, String)> = state
        .installed_apps
        .iter()
        .map(|app| (app.id.0.clone(), app.display_name.clone()))
        .collect();
    apps.push(none_criteria());
    questions.insert(
        "application".into(),
        choice(
            "If LaunchApp is chosen, select one installed application. Otherwise select __none__.",
            apps,
        ),
    );
    let mut visuals: Vec<(String, String)> = state
        .artifacts
        .iter()
        .filter(|artifact| {
            matches!(
                artifact.kind,
                ArtifactKind::VisualInstruction | ArtifactKind::UserGoal
            )
        })
        .map(|artifact| (artifact.id.0.clone(), artifact.value.clone()))
        .collect();
    visuals.push(none_criteria());
    questions.insert("visual_artifact".into(), choice("If VisualAction is chosen, select an existing concise visual instruction. The original user goal is valid only when it is itself one low-level visible GUI action. Otherwise select __none__.", visuals));
    let mut text: Vec<(String, String)> = state
        .artifacts
        .by_kind(ArtifactKind::TextToType)
        .into_iter()
        .map(|artifact| (artifact.id.0.clone(), artifact.value.clone()))
        .collect();
    text.push(none_criteria());
    questions.insert("type_artifact".into(), choice("If TypeArtifact is chosen, select an existing text artifact. Otherwise select __none__.", text));
    let mut messages: Vec<(String, String)> = state
        .artifacts
        .iter()
        .filter(|artifact| {
            matches!(
                artifact.kind,
                ArtifactKind::AssistantReply | ArtifactKind::ClarificationQuestion
            )
        })
        .map(|artifact| (artifact.id.0.clone(), artifact.value.clone()))
        .collect();
    messages.push(none_criteria());
    questions.insert("user_message_artifact".into(), choice("If AskUser or ReplyToUser is chosen, select the corresponding existing message artifact. Otherwise select __none__.", messages));
    let mut keys: Vec<(String, String)> = Key::choices()
        .into_iter()
        .map(|key| {
            (
                key.choice_id().into(),
                format!("Supported key {}", key.choice_id()),
            )
        })
        .collect();
    keys.push(none_criteria());
    questions.insert(
        "key".into(),
        choice(
            "If PressKey is chosen, select one supported key. Otherwise select __none__.",
            keys,
        ),
    );
    let mut directions: Vec<(String, String)> = ScrollDirection::all()
        .into_iter()
        .map(|direction| {
            (
                direction.choice_id().into(),
                format!("Scroll {}", direction.choice_id()),
            )
        })
        .collect();
    directions.push(none_criteria());
    questions.insert(
        "scroll_direction".into(),
        choice(
            "If Scroll is chosen, select a direction. Otherwise select __none__.",
            directions,
        ),
    );
    let mut durations: Vec<(String, String)> = fixed_wait_durations()
        .into_iter()
        .map(|duration| {
            (
                duration.as_millis().to_string(),
                format!("Wait {} ms", duration.as_millis()),
            )
        })
        .collect();
    durations.push(none_criteria());
    questions.insert(
        "wait_duration".into(),
        choice(
            "If Wait is chosen, select one fixed duration. Otherwise select __none__.",
            durations,
        ),
    );
    let mut requests: Vec<(String, String)> = TextRequestKind::all()
        .into_iter()
        .map(|kind| {
            (
                kind.choice_id().into(),
                format!("Request {}", kind.choice_id()),
            )
        })
        .collect();
    requests.push(none_criteria());
    questions.insert("text_request_kind".into(), choice("If RequestText is chosen, select the language-only request kind. Otherwise select __none__.", requests));
    questions.insert("risk_level".into(), choice("Rate the risk of the selected next action. This can raise protection but never lowers Rust policy.", [("low".into(), "Reversible or no side effect.".into()), ("medium".into(), "Desktop input or app launch with moderate consequence.".into()), ("high".into(), "Could send text or cause a consequential change.".into())]));
    questions
}

pub fn resolve(decision: &JevDecision, state: &AgentState) -> Result<AgentAction, DecisionError> {
    let capability = Capability::from_choice(&decision.action.choice)
        .ok_or_else(|| DecisionError::Resolution("action is outside closed set".into()))?;
    let require = |value: &str, field: &str| {
        if value == NONE {
            Err(DecisionError::Resolution(format!(
                "{capability:?} requires {field}"
            )))
        } else {
            Ok(value.to_string())
        }
    };
    match capability {
        Capability::LaunchApp => {
            let id = require(&decision.application.choice, "application")?;
            state
                .installed_apps
                .iter()
                .find(|app| app.id.0 == id)
                .map(|app| AgentAction::LaunchApp {
                    app_id: app.id.clone(),
                })
                .ok_or_else(|| {
                    DecisionError::Resolution("chosen application is unavailable".into())
                })
        }
        Capability::VisualAction => {
            let id = ArtifactId(require(
                &decision.visual_artifact.choice,
                "visual_artifact",
            )?);
            state
                .artifacts
                .get(&id)
                .filter(|artifact| {
                    matches!(
                        artifact.kind,
                        ArtifactKind::VisualInstruction | ArtifactKind::UserGoal
                    )
                })
                .map(|_| AgentAction::VisualAction {
                    instruction_artifact: id,
                })
                .ok_or_else(|| DecisionError::Resolution("visual artifact is invalid".into()))
        }
        Capability::TypeArtifact => {
            let id = ArtifactId(require(&decision.type_artifact.choice, "type_artifact")?);
            state
                .artifacts
                .get(&id)
                .filter(|artifact| artifact.kind == ArtifactKind::TextToType)
                .map(|_| AgentAction::TypeArtifact { artifact: id })
                .ok_or_else(|| DecisionError::Resolution("type artifact is invalid".into()))
        }
        Capability::PressKey => Key::from_choice(&require(&decision.key.choice, "key")?)
            .map(|key| AgentAction::PressKey { key })
            .ok_or_else(|| DecisionError::Resolution("unsupported key".into())),
        Capability::Hotkey => Err(DecisionError::Resolution(
            "Hotkey needs a future closed-set shortcut selection".into(),
        )),
        Capability::Scroll => ScrollDirection::from_choice(&require(
            &decision.scroll_direction.choice,
            "scroll_direction",
        )?)
        .map(|direction| AgentAction::Scroll { direction })
        .ok_or_else(|| DecisionError::Resolution("unsupported scroll direction".into())),
        Capability::RequestText => TextRequestKind::from_choice(&require(
            &decision.text_request_kind.choice,
            "text_request_kind",
        )?)
        .map(|kind| AgentAction::RequestText { kind })
        .ok_or_else(|| DecisionError::Resolution("unsupported text request".into())),
        Capability::AskUser => {
            let id = ArtifactId(
                require(
                    &decision.user_message_artifact.choice,
                    "user_message_artifact",
                )?
                .into(),
            );
            state
                .artifacts
                .get(&id)
                .filter(|artifact| artifact.kind == ArtifactKind::ClarificationQuestion)
                .map(|_| AgentAction::AskUser { artifact: id })
                .ok_or_else(|| {
                    DecisionError::Resolution("clarification artifact is invalid".into())
                })
        }
        Capability::ReplyToUser => {
            let id = ArtifactId(
                require(
                    &decision.user_message_artifact.choice,
                    "user_message_artifact",
                )?
                .into(),
            );
            state
                .artifacts
                .get(&id)
                .filter(|artifact| artifact.kind == ArtifactKind::AssistantReply)
                .map(|_| AgentAction::ReplyToUser { artifact: id })
                .ok_or_else(|| DecisionError::Resolution("reply artifact is invalid".into()))
        }
        Capability::Wait => {
            let value = require(&decision.wait_duration.choice, "wait_duration")?
                .parse::<u64>()
                .map_err(|_| DecisionError::Resolution("invalid wait duration".into()))?;
            fixed_wait_durations()
                .into_iter()
                .any(|duration| duration.as_millis() == value as u128)
                .then_some(AgentAction::Wait { duration_ms: value })
                .ok_or_else(|| {
                    DecisionError::Resolution("wait duration is outside closed set".into())
                })
        }
        Capability::Done => Ok(AgentAction::Done),
        Capability::Abort => Ok(AgentAction::Abort),
    }
}

impl JevDecision {
    pub fn risk(&self) -> Result<RiskLevel, DecisionError> {
        RiskLevel::from_choice(&self.risk_level.choice).ok_or_else(|| {
            DecisionError::InvalidResponse("risk_level is outside closed set".into())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{AppId, CapabilityAvailability, DesktopState};
    fn state() -> AgentState {
        AgentState::new(
            "Open Example Chat and send a message",
            CapabilityAvailability {
                vision: true,
                text_provider: true,
                desktop_input: true,
                app_catalog: true,
            },
            DesktopState {
                live_capture: true,
                execute_enabled: false,
            },
            vec![crate::action::AppInfo {
                id: AppId("example.desktop".into()),
                display_name: "Example Chat".into(),
                description: None,
            }],
        )
    }
    fn answer(choice: &str) -> ChoiceAnswer {
        ChoiceAnswer {
            choice: choice.into(),
            probabilities: BTreeMap::new(),
            confidence: 0.9,
        }
    }
    fn decision(action: &str) -> JevDecision {
        JevDecision {
            action: answer(action),
            application: answer(NONE),
            visual_artifact: answer(NONE),
            type_artifact: answer(NONE),
            user_message_artifact: answer(NONE),
            key: answer(NONE),
            scroll_direction: answer(NONE),
            wait_duration: answer(NONE),
            text_request_kind: answer(NONE),
            risk_level: answer("low"),
            request_id: None,
            latency_ms: 1.0,
            model: None,
        }
    }
    #[test]
    fn requests_have_speculative_closed_sets() {
        let request = build_request(&state(), "jev");
        assert!(request.questions.contains_key("application"));
        assert!(request.questions.contains_key("text_request_kind"));
        assert!(request.questions["application"].criteria.contains_key(NONE));
        assert!(
            !request.questions["action"]
                .criteria
                .contains_key(Capability::ReplyToUser.choice_id())
        );
        assert!(
            !request.questions["action"]
                .criteria
                .contains_key(Capability::AskUser.choice_id())
        );
    }
    #[test]
    fn launch_without_app_is_rejected() {
        assert!(resolve(&decision("launch_app"), &state()).is_err());
    }
    #[test]
    fn missing_visual_artifact_is_rejected() {
        assert!(resolve(&decision("visual_action"), &state()).is_err());
    }
}

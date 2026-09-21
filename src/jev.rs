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
    ActionHintKind, ActionSource, AgentAction, AgentState, Capability, CommitOperation,
    CommitOperationResolved, GuidanceArtifactContent, ResolvedAction,
};

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
impl ChoiceAnswer {
    pub fn selected_probability(&self) -> Option<f64> {
        self.probabilities.get(&self.choice).copied()
    }
    pub fn runner_up_probability(&self) -> Option<f64> {
        self.probabilities
            .iter()
            .filter(|(choice, _)| *choice != &self.choice)
            .map(|(_, value)| *value)
            .max_by(f64::total_cmp)
    }
    pub fn margin(&self) -> Option<f64> {
        Some(self.selected_probability()? - self.runner_up_probability()?)
    }
}

#[derive(Clone, Debug)]
pub struct JevDecision {
    pub action: ChoiceAnswer,
    pub application_intent: Option<ChoiceAnswer>,
    pub visual_intent: Option<ChoiceAnswer>,
    pub type_intent: Option<ChoiceAnswer>,
    pub key_intent: Option<ChoiceAnswer>,
    pub hotkey_intent: Option<ChoiceAnswer>,
    pub scroll_intent: Option<ChoiceAnswer>,
    pub text_request: Option<ChoiceAnswer>,
    pub wait_intent: Option<ChoiceAnswer>,
    pub commit_intent: Option<ChoiceAnswer>,
    pub ask_user_intent: Option<ChoiceAnswer>,
    pub reply_user_intent: Option<ChoiceAnswer>,
    pub request_id: Option<String>,
    pub latency_ms: f64,
    pub model: Option<String>,
    pub candidates: DecisionCandidates,
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

#[derive(Clone, Debug, Serialize)]
pub struct IntentCandidate {
    pub description: String,
    pub resolved: ResolvedAction,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct DecisionCandidates {
    pub revision: (u64, u64, u64),
    pub actions: BTreeMap<String, String>,
    pub application_intents: BTreeMap<String, IntentCandidate>,
    pub visual_intents: BTreeMap<String, IntentCandidate>,
    pub type_intents: BTreeMap<String, IntentCandidate>,
    pub key_intents: BTreeMap<String, IntentCandidate>,
    pub hotkey_intents: BTreeMap<String, IntentCandidate>,
    pub scroll_intents: BTreeMap<String, IntentCandidate>,
    pub text_requests: BTreeMap<String, IntentCandidate>,
    pub wait_intents: BTreeMap<String, IntentCandidate>,
    pub commit_intents: BTreeMap<String, IntentCandidate>,
    pub ask_user_intents: BTreeMap<String, IntentCandidate>,
    pub reply_user_intents: BTreeMap<String, IntentCandidate>,
}
impl DecisionCandidates {
    pub fn has_actionable(&self) -> bool {
        self.actions.keys().any(|id| {
            id != Capability::Abort.choice_id()
                && id != Capability::Done.choice_id()
                && id != Capability::RequestReplan.choice_id()
        })
    }
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
        let candidates = build_candidates(state);
        let request = build_request(state, &candidates, &self.config.model);
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
        let answer = |name: &str, required: bool| -> Result<Option<ChoiceAnswer>, DecisionError> {
            let Some(wire) = parsed.answers.get(name) else {
                return if required {
                    Err(DecisionError::InvalidResponse(format!(
                        "missing answers.{name}"
                    )))
                } else {
                    Ok(None)
                };
            };
            if !wire.confidence.is_finite() || !(0.0..=1.0).contains(&wire.confidence) {
                return Err(DecisionError::InvalidResponse(format!(
                    "answers.{name}.confidence is invalid"
                )));
            }
            Ok(Some(ChoiceAnswer {
                choice: wire.choice.clone(),
                probabilities: wire.probabilities.clone(),
                confidence: wire.confidence,
            }))
        };
        Ok(JevDecision {
            action: answer("action", true)?.expect("required"),
            application_intent: answer("application_intent", false)?,
            visual_intent: answer("visual_intent", false)?,
            type_intent: answer("type_intent", false)?,
            key_intent: answer("key_intent", false)?,
            hotkey_intent: answer("hotkey_intent", false)?,
            scroll_intent: answer("scroll_intent", false)?,
            text_request: answer("text_request", false)?,
            wait_intent: answer("wait_intent", false)?,
            commit_intent: answer("commit_intent", false)?,
            ask_user_intent: answer("ask_user_intent", false)?,
            reply_user_intent: answer("reply_user_intent", false)?,
            request_id: parsed.request_id,
            latency_ms: started.elapsed().as_secs_f64() * 1000.0,
            model: parsed.model,
            candidates,
        })
    }
    async fn test_connection(&self) -> Result<(), DecisionError> {
        let state = AgentState::new(
            "connection test",
            crate::action::CapabilityAvailability {
                vision: false,
                text_provider: false,
                desktop_input: false,
                text_input: false,
                app_catalog: false,
            },
            crate::action::DesktopState {
                live_capture: false,
                execute_enabled: false,
                last_grounded_frame: None,
            },
            vec![],
        );
        self.decide(&state).await.map(|_| ())
    }
}

fn intent(
    _id: String,
    description: String,
    action: AgentAction,
    source: ActionSource,
    terminal: bool,
) -> IntentCandidate {
    IntentCandidate {
        description,
        resolved: ResolvedAction {
            action,
            source,
            hint_id: None,
            terminal,
        },
    }
}
fn hint_intent(
    hint: &crate::action::ActionHint,
    description: String,
    action: AgentAction,
) -> IntentCandidate {
    let mut item = intent(
        hint.id.0.clone(),
        description,
        action,
        ActionSource::Guidance(hint.id.clone()),
        hint.terminal,
    );
    item.resolved.hint_id = Some(hint.id.clone());
    item
}
fn attempted(state: &AgentState, action: &AgentAction) -> bool {
    state
        .recent_history
        .iter()
        .any(|record| &record.selected_action == action)
}

/// Builds only actions whose operands and backends are available now.
pub fn build_candidates(state: &AgentState) -> DecisionCandidates {
    let mut candidates = DecisionCandidates {
        revision: (
            state.state_revision,
            state.observation_revision,
            state.guidance_revision,
        ),
        ..Default::default()
    };
    candidates.actions.insert(
        Capability::Abort.choice_id().into(),
        "Stop because continuing is impossible or unsafe.".into(),
    );
    if state.guidance.is_none() {
        let plan_capability = if state.replan_reason.is_some() {
            Capability::RequestReplan
        } else {
            Capability::RequestPlan
        };
        let plan_description = if state.replan_reason.is_some() {
            "Request replacement guidance because recorded evidence invalidated the current approach."
        } else {
            "Request non-authoritative guidance and artifacts for an objective that is not yet represented by concrete operands."
        };
        candidates
            .actions
            .insert(plan_capability.choice_id().into(), plan_description.into());
        if state.available_capabilities.vision
            && !attempted(
                state,
                &AgentAction::VisualAction {
                    instruction_artifact: state.goal.clone(),
                },
            )
        {
            let action = AgentAction::VisualAction {
                instruction_artifact: state.goal.clone(),
            };
            candidates.visual_intents.insert("goal_visual".into(), intent("goal_visual".into(), "Use the user goal itself only when it is already one atomic visible pointer instruction.".into(), action, ActionSource::FastPath, true));
            candidates.actions.insert(Capability::VisualAction.choice_id().into(), "Use only when the user goal itself is one concrete visible pointer interaction; otherwise request guidance.".into());
        }
        if state.available_capabilities.app_catalog {
            for app in &state.installed_apps {
                let action = AgentAction::LaunchApp {
                    app_id: app.id.clone(),
                };
                if !attempted(state, &action) {
                    candidates.application_intents.insert(
                        app.id.0.clone(),
                        intent(
                            app.id.0.clone(),
                            format!("Launch installed application {}", app.display_name),
                            action,
                            ActionSource::FastPath,
                            true,
                        ),
                    );
                }
            }
            if !candidates.application_intents.is_empty() {
                candidates.actions.insert(Capability::LaunchApp.choice_id().into(), "Use only when the user explicitly asks to open or launch an application; preserve the current desktop otherwise.".into());
            }
        }
        return candidates;
    }
    let runtime = state.guidance.as_ref().expect("checked");
    // A typed failure invalidates the old guidance. Stale hints are not
    // candidates beside RequestReplan: replanning is a state transition, not
    // an optional extra action.
    if state.replan_reason.is_some() {
        candidates.actions.insert(
            Capability::RequestReplan.choice_id().into(),
            "Request replacement guidance because recorded evidence invalidated the current guidance."
                .into(),
        );
        return candidates;
    }
    for spec in runtime.pending() {
        let GuidanceArtifactContent::Generate { kind, purpose, .. } = &spec.content else {
            continue;
        };
        if state.available_capabilities.text_provider {
            candidates.text_requests.insert(
                spec.id.0.clone(),
                intent(
                    spec.id.0.clone(),
                    format!("Generate {}: {}", kind.choice_id(), purpose),
                    AgentAction::RequestText {
                        artifact: spec.id.clone(),
                        kind: kind.clone(),
                    },
                    ActionSource::System,
                    false,
                ),
            );
            candidates.actions.insert(
                Capability::RequestText.choice_id().into(),
                "Generate only the pending text artifact selected by text_request.".into(),
            );
        }
    }
    for hint in runtime.available_hints() {
        match &hint.action {
            ActionHintKind::LaunchApp if state.available_capabilities.app_catalog => {
                for app in &state.installed_apps {
                    let action = AgentAction::LaunchApp {
                        app_id: app.id.clone(),
                    };
                    let id = format!("{}:{}", hint.id.0, app.id.0);
                    candidates.application_intents.insert(
                        id.clone(),
                        hint_intent(
                            hint,
                            format!("{} via {}", hint.id.0, app.display_name),
                            action,
                        ),
                    );
                }
            }
            ActionHintKind::VisualAction { artifact } if state.available_capabilities.vision => {
                if let Some(id) = runtime.artifact(artifact) {
                    candidates.visual_intents.insert(
                        hint.id.0.clone(),
                        hint_intent(
                            hint,
                            format!("Ground visual artifact {}", artifact.0),
                            AgentAction::VisualAction {
                                instruction_artifact: id.clone(),
                            },
                        ),
                    );
                }
            }
            ActionHintKind::TypeArtifact { artifact }
                if state.available_capabilities.text_input =>
            {
                if let Some(id) = runtime.artifact(artifact) {
                    candidates.type_intents.insert(
                        hint.id.0.clone(),
                        hint_intent(
                            hint,
                            format!("Type artifact {}", artifact.0),
                            AgentAction::TypeArtifact {
                                artifact: id.clone(),
                            },
                        ),
                    );
                }
            }
            ActionHintKind::PressKey { key } if state.available_capabilities.desktop_input => {
                candidates.key_intents.insert(
                    hint.id.0.clone(),
                    hint_intent(
                        hint,
                        format!("Press {}", key.choice_id()),
                        AgentAction::PressKey { key: key.clone() },
                    ),
                );
            }
            ActionHintKind::Hotkey { shortcut } if state.available_capabilities.desktop_input => {
                candidates.hotkey_intents.insert(
                    hint.id.0.clone(),
                    hint_intent(
                        hint,
                        format!("Use {}", shortcut.choice_id()),
                        AgentAction::Hotkey {
                            shortcut: shortcut.clone(),
                        },
                    ),
                );
            }
            ActionHintKind::Scroll { direction } if state.available_capabilities.desktop_input => {
                candidates.scroll_intents.insert(
                    hint.id.0.clone(),
                    hint_intent(
                        hint,
                        format!("Scroll {}", direction.choice_id()),
                        AgentAction::Scroll {
                            direction: direction.clone(),
                        },
                    ),
                );
            }
            ActionHintKind::Wait { duration } => {
                candidates.wait_intents.insert(
                    hint.id.0.clone(),
                    hint_intent(
                        hint,
                        format!("Wait {} ms", duration.milliseconds()),
                        AgentAction::Wait {
                            duration: duration.clone(),
                        },
                    ),
                );
            }
            ActionHintKind::AskUser { artifact } => {
                if let Some(id) = runtime.artifact(artifact) {
                    candidates.ask_user_intents.insert(
                        hint.id.0.clone(),
                        hint_intent(
                            hint,
                            "Ask the prepared clarification question".into(),
                            AgentAction::AskUser {
                                artifact: id.clone(),
                            },
                        ),
                    );
                }
            }
            ActionHintKind::ReplyToUser { artifact } => {
                if let Some(id) = runtime.artifact(artifact) {
                    candidates.reply_user_intents.insert(
                        hint.id.0.clone(),
                        hint_intent(
                            hint,
                            "Show the prepared reply".into(),
                            AgentAction::ReplyToUser {
                                artifact: id.clone(),
                            },
                        ),
                    );
                }
            }
            ActionHintKind::Commit { preview, operation }
                if state.available_capabilities.desktop_input =>
            {
                if let Some(preview) = runtime.artifact(preview) {
                    let operation = match operation {
                        CommitOperation::VisualAction { artifact } => runtime
                            .artifact(artifact)
                            .map(|id| CommitOperationResolved::VisualAction {
                                instruction_artifact: id.clone(),
                            }),
                        CommitOperation::PressKey { key } => {
                            Some(CommitOperationResolved::PressKey { key: key.clone() })
                        }
                        CommitOperation::Hotkey { shortcut } => {
                            Some(CommitOperationResolved::Hotkey {
                                shortcut: shortcut.clone(),
                            })
                        }
                    };
                    if let Some(operation) = operation {
                        candidates.commit_intents.insert(
                            hint.id.0.clone(),
                            hint_intent(
                                hint,
                                "Perform the explicitly consequential operation".into(),
                                AgentAction::Commit {
                                    preview_artifact: preview.clone(),
                                    operation,
                                },
                            ),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    if !candidates.application_intents.is_empty() {
        candidates.actions.insert(
            Capability::LaunchApp.choice_id().into(),
            "Launch an installed application selected by application_intent.".into(),
        );
    }
    if !candidates.visual_intents.is_empty() {
        candidates.actions.insert(
            Capability::VisualAction.choice_id().into(),
            "Ground and click the selected visible instruction.".into(),
        );
    }
    if !candidates.type_intents.is_empty() {
        candidates.actions.insert(
            Capability::TypeArtifact.choice_id().into(),
            "Enter the selected existing text artifact.".into(),
        );
    }
    if !candidates.key_intents.is_empty() {
        candidates.actions.insert(
            Capability::PressKey.choice_id().into(),
            "Send the selected hinted non-text key.".into(),
        );
    }
    if !candidates.hotkey_intents.is_empty() {
        candidates.actions.insert(
            Capability::Hotkey.choice_id().into(),
            "Send the selected named shortcut.".into(),
        );
    }
    if !candidates.scroll_intents.is_empty() {
        candidates.actions.insert(
            Capability::Scroll.choice_id().into(),
            "Perform the selected hinted scroll.".into(),
        );
    }
    if !candidates.wait_intents.is_empty() {
        candidates.actions.insert(
            Capability::Wait.choice_id().into(),
            "Wait only for the selected hinted duration.".into(),
        );
    }
    if !candidates.commit_intents.is_empty() {
        candidates.actions.insert(
            Capability::Commit.choice_id().into(),
            "Prepare the selected explicitly consequential operation for confirmation.".into(),
        );
    }
    if !candidates.ask_user_intents.is_empty() {
        candidates.actions.insert(
            Capability::AskUser.choice_id().into(),
            "Ask the selected prepared clarification question and pause.".into(),
        );
    }
    if !candidates.reply_user_intents.is_empty() {
        candidates.actions.insert(
            Capability::ReplyToUser.choice_id().into(),
            "Show the selected prepared reply.".into(),
        );
    }
    if state.terminal_evidence && state.replan_reason.is_none() {
        candidates.actions.insert(
            Capability::Done.choice_id().into(),
            "Finish only as operationally complete and visually unverified.".into(),
        );
    }
    candidates
}

fn criteria(items: &BTreeMap<String, IntentCandidate>) -> BTreeMap<String, Value> {
    items
        .iter()
        .map(|(id, item)| (id.clone(), Value::String(item.description.clone())))
        .collect()
}
fn question(instructions: &str, values: BTreeMap<String, Value>) -> ChoiceQuestion {
    ChoiceQuestion {
        kind: "choice",
        instructions: instructions.into(),
        criteria: values,
    }
}
fn add_question(
    questions: &mut BTreeMap<String, ChoiceQuestion>,
    name: &str,
    instructions: &str,
    items: &BTreeMap<String, IntentCandidate>,
) {
    if !items.is_empty() {
        questions.insert(name.into(), question(instructions, criteria(items)));
    }
}

fn build_request(state: &AgentState, candidates: &DecisionCandidates, model: &str) -> JevRequest {
    let goal = state
        .artifacts
        .get(&state.goal)
        .map(|item| item.value.clone())
        .unwrap_or_default();
    let artifacts: Vec<Value> = state.artifacts.iter().take(16).map(|item| serde_json::json!({"id":item.id,"kind":item.kind,"text":item.value,"source":item.source})).collect();
    let history: Vec<Value> = state.recent_history.iter().map(|item| serde_json::json!({"action":item.selected_action,"hint_id":item.hint_id,"status":item.status,"input":item.input_result,"verification":item.verification})).collect();
    let compact_state = serde_json::json!({"goal":{"artifact_id":state.goal,"text":goal},"guidance":state.guidance,"artifacts":artifacts,"recent_history":history,"last_execution":state.last_execution,"replan_reason":state.replan_reason,"completion":state.completion,"availability":state.available_capabilities,"desktop":state.desktop_state,"candidate_revision":candidates.revision,"installed_apps":state.installed_apps});
    let mut questions = BTreeMap::new();
    questions.insert("action".into(), question("Choose exactly one currently executable bounded capability. Rust owns the workflow; choose one operation, never a plan. Select VisualAction over the user goal only if that goal is exactly one atomic visible pointer interaction. If it asks to click and then type, search, send, open, navigate, or otherwise contains more than that one interaction, select RequestPlan instead; Rust will not split free text into actions.", candidates.actions.iter().map(|(id, description)| (id.clone(), Value::String(description.clone()))).collect()));
    add_question(
        &mut questions,
        "application_intent",
        "Select an actually launchable application intent for a possible LaunchApp action.",
        &candidates.application_intents,
    );
    add_question(
        &mut questions,
        "visual_intent",
        "Select an actually groundable visual intent for a possible VisualAction.",
        &candidates.visual_intents,
    );
    add_question(
        &mut questions,
        "type_intent",
        "Select an existing text artifact intent for a possible TypeArtifact.",
        &candidates.type_intents,
    );
    add_question(
        &mut questions,
        "key_intent",
        "Select a hinted non-text key for a possible PressKey.",
        &candidates.key_intents,
    );
    add_question(
        &mut questions,
        "hotkey_intent",
        "Select a named shortcut for a possible Hotkey.",
        &candidates.hotkey_intents,
    );
    add_question(
        &mut questions,
        "scroll_intent",
        "Select a hinted scroll operation.",
        &candidates.scroll_intents,
    );
    add_question(
        &mut questions,
        "text_request",
        "Select the pending artifact to generate. Generate text only; do not choose another action.",
        &candidates.text_requests,
    );
    add_question(
        &mut questions,
        "wait_intent",
        "Select a hinted wait duration.",
        &candidates.wait_intents,
    );
    add_question(
        &mut questions,
        "commit_intent",
        "Select the explicitly consequential operation that must be confirmed before execution.",
        &candidates.commit_intents,
    );
    add_question(
        &mut questions,
        "ask_user_intent",
        "Select the prepared clarification question for a possible AskUser action.",
        &candidates.ask_user_intents,
    );
    add_question(
        &mut questions,
        "reply_user_intent",
        "Select the prepared reply for a possible ReplyToUser action.",
        &candidates.reply_user_intents,
    );
    JevRequest {
        model: model.into(),
        state: compact_state,
        questions,
    }
}

fn selected(
    map: &BTreeMap<String, IntentCandidate>,
    answer: Option<&ChoiceAnswer>,
    field: &str,
) -> Result<ResolvedAction, DecisionError> {
    let id = answer
        .ok_or_else(|| DecisionError::Resolution(format!("selected action requires {field}")))?
        .choice
        .as_str();
    map.get(id)
        .map(|item| item.resolved.clone())
        .ok_or_else(|| DecisionError::Resolution(format!("selected unavailable {field}")))
}
pub fn resolve(decision: &JevDecision) -> Result<ResolvedAction, DecisionError> {
    let capability = Capability::from_choice(&decision.action.choice)
        .ok_or_else(|| DecisionError::Resolution("action is outside closed set".into()))?;
    if !decision
        .candidates
        .actions
        .contains_key(&decision.action.choice)
    {
        return Err(DecisionError::Resolution(
            "chosen action is not currently available".into(),
        ));
    }
    let direct = |action| {
        Ok(ResolvedAction {
            action,
            source: ActionSource::System,
            hint_id: None,
            terminal: false,
        })
    };
    match capability {
        Capability::LaunchApp => selected(
            &decision.candidates.application_intents,
            decision.application_intent.as_ref(),
            "application_intent",
        ),
        Capability::VisualAction => selected(
            &decision.candidates.visual_intents,
            decision.visual_intent.as_ref(),
            "visual_intent",
        ),
        Capability::TypeArtifact => selected(
            &decision.candidates.type_intents,
            decision.type_intent.as_ref(),
            "type_intent",
        ),
        Capability::PressKey => selected(
            &decision.candidates.key_intents,
            decision.key_intent.as_ref(),
            "key_intent",
        ),
        Capability::Hotkey => selected(
            &decision.candidates.hotkey_intents,
            decision.hotkey_intent.as_ref(),
            "hotkey_intent",
        ),
        Capability::Scroll => selected(
            &decision.candidates.scroll_intents,
            decision.scroll_intent.as_ref(),
            "scroll_intent",
        ),
        Capability::RequestText => selected(
            &decision.candidates.text_requests,
            decision.text_request.as_ref(),
            "text_request",
        ),
        Capability::Wait => selected(
            &decision.candidates.wait_intents,
            decision.wait_intent.as_ref(),
            "wait_intent",
        ),
        Capability::Commit => selected(
            &decision.candidates.commit_intents,
            decision.commit_intent.as_ref(),
            "commit_intent",
        ),
        Capability::AskUser => selected(
            &decision.candidates.ask_user_intents,
            decision.ask_user_intent.as_ref(),
            "ask_user_intent",
        ),
        Capability::ReplyToUser => selected(
            &decision.candidates.reply_user_intents,
            decision.reply_user_intent.as_ref(),
            "reply_user_intent",
        ),
        Capability::RequestPlan => direct(AgentAction::RequestPlan),
        Capability::RequestReplan => direct(AgentAction::RequestReplan),
        Capability::Done => direct(AgentAction::Done),
        Capability::Abort => direct(AgentAction::Abort),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{CapabilityAvailability, DesktopState, TaskGuidance};
    fn state() -> AgentState {
        AgentState::new(
            "Click the visible button",
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
        )
    }
    #[test]
    fn no_guidance_has_only_fast_paths_and_plan() {
        let candidates = build_candidates(&state());
        assert!(candidates.actions.contains_key("request_plan"));
        assert!(candidates.actions.contains_key("visual_action"));
        assert!(!candidates.actions.contains_key("press_key"));
    }
    #[test]
    fn failed_fast_path_requests_a_replan_even_without_guidance() {
        let mut state = state();
        state.set_replan(crate::action::ReplanReason::VerificationFailed { hint_id: None });
        let candidates = build_candidates(&state);
        assert!(candidates.actions.contains_key("request_replan"));
        assert!(!candidates.actions.contains_key("request_plan"));
    }
    #[test]
    fn press_key_requires_a_hint_and_never_offers_letters() {
        let mut state = state();
        state.set_guidance(TaskGuidance::parse(r#"{"objective":"test","artifacts":[],"hints":[{"id":"enter","action":{"type":"press_key","key":"enter"}}]}"#).unwrap());
        let candidates = build_candidates(&state);
        assert_eq!(candidates.key_intents.len(), 1);
        assert!(candidates.actions.contains_key("press_key"));
        assert!(
            !candidates
                .key_intents
                .values()
                .any(|item| item.description.contains(" a"))
        );
    }
    #[test]
    fn generated_artifact_offers_request_not_type() {
        let mut state = state();
        state.set_guidance(TaskGuidance::parse(r#"{"objective":"test","artifacts":[{"id":"text","kind":"text_to_type","content":{"type":"generate","kind":"compose_text","purpose":"Write text"}}],"hints":[{"id":"type","action":{"type":"type_artifact","artifact":"text"}}]}"#).unwrap());
        let candidates = build_candidates(&state);
        assert!(candidates.actions.contains_key("request_text"));
        assert!(!candidates.actions.contains_key("type_artifact"));
    }

    #[test]
    fn consumed_hint_is_not_offered_again() {
        let mut state = state();
        state.set_guidance(
            TaskGuidance::parse(r#"{"objective":"test","artifacts":[],"hints":[{"id":"enter","action":{"type":"press_key","key":"enter"}}]}"#).unwrap(),
        );
        state.mark_hint(
            &crate::action::GuidanceHintId("enter".into()),
            crate::action::HintStatus::Succeeded { step: 1 },
        );
        assert!(!build_candidates(&state).actions.contains_key("press_key"));
    }

    #[test]
    fn request_replan_requires_recorded_reason() {
        let mut state = state();
        state.set_guidance(TaskGuidance::parse(r#"{"objective":"test","artifacts":[],"hints":[{"id":"enter","action":{"type":"press_key","key":"enter"}}]}"#).unwrap());
        assert!(
            !build_candidates(&state)
                .actions
                .contains_key("request_replan")
        );
        state.set_replan(crate::action::ReplanReason::GuidanceExhausted);
        let candidates = build_candidates(&state);
        assert!(candidates.actions.contains_key("request_replan"));
        assert!(!candidates.actions.contains_key("press_key"));
    }

    #[test]
    fn done_requires_terminal_operational_evidence() {
        let mut state = state();
        state.set_guidance(TaskGuidance::parse(r#"{"objective":"test","artifacts":[],"hints":[{"id":"submit","action":{"type":"press_key","key":"enter"},"terminal":true}]}"#).unwrap());
        assert!(!build_candidates(&state).actions.contains_key("done"));
        state.mark_hint(
            &crate::action::GuidanceHintId("submit".into()),
            crate::action::HintStatus::Succeeded { step: 1 },
        );
        state.terminal_evidence = true;
        assert!(build_candidates(&state).actions.contains_key("done"));
    }

    #[test]
    fn every_offered_argument_action_has_a_real_closed_set() {
        let mut state = state();
        state.set_guidance(TaskGuidance::parse(r#"{"objective":"test","artifacts":[{"id":"target","kind":"visual_instruction","content":{"type":"literal","value":"Click visible item"}},{"id":"text","kind":"text_to_type","content":{"type":"literal","value":"hello"}}],"hints":[{"id":"click","action":{"type":"visual_action","artifact":"target"}},{"id":"type","action":{"type":"type_artifact","artifact":"text"}},{"id":"enter","action":{"type":"press_key","key":"enter"}}]}"#).unwrap());
        let candidates = build_candidates(&state);
        assert_eq!(
            candidates.actions.contains_key("visual_action"),
            !candidates.visual_intents.is_empty()
        );
        assert_eq!(
            candidates.actions.contains_key("type_artifact"),
            !candidates.type_intents.is_empty()
        );
        assert_eq!(
            candidates.actions.contains_key("press_key"),
            !candidates.key_intents.is_empty()
        );
        assert!(!candidates.actions.contains_key("scroll"));
        assert!(!candidates.actions.contains_key("wait"));
    }

    #[test]
    fn unavailable_clipboard_removes_type_artifact_candidate() {
        let mut state = state();
        state.available_capabilities.text_input = false;
        state.set_guidance(TaskGuidance::parse(r#"{"objective":"test","artifacts":[{"id":"text","kind":"text_to_type","content":{"type":"literal","value":"Olá 😊"}}],"hints":[{"id":"type","action":{"type":"type_artifact","artifact":"text"}}]}"#).unwrap());
        assert!(
            !build_candidates(&state)
                .actions
                .contains_key("type_artifact")
        );
    }
}

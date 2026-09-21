use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use reqwest::Client;
use tokio::sync::{Semaphore, oneshot};

use crate::{
    action::{
        ActionRecord, ActionStatus, AgentAction, AgentState, ArtifactKind, ArtifactSource,
        CapabilityAvailability, DesktopState, InputResult, VerificationResult,
    },
    app_catalog::{AppCatalog, GioAppCatalog},
    config::Config,
    debug::DebugWriter,
    desktop::{Frame, PortalSession, TextInputBackend},
    jev::{DecisionProvider, JevClient, JevConfig, resolve},
    policy::{DecisionEvidence, PolicyDecision, PolicyEngine},
    text_provider::{OpenAiCompatibleTextProvider, TextProvider, TextRequest},
    vision::{GroundingResult, VisionProvider, VocaelaVisionProvider},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineState {
    Idle,
    Decide,
    Resolve,
    Observe,
    PolicyCheck,
    Execute,
    Verify,
    RequestingText,
    AwaitingTextProviderSetup,
    AwaitingUser,
    AwaitingConfirmation,
    Completed,
    Failed,
    Cancelled,
}
#[derive(Debug)]
pub struct ConfirmationRequest {
    pub action: String,
    pub preview: Option<String>,
    pub responder: Arc<std::sync::Mutex<Option<oneshot::Sender<bool>>>>,
}
impl ConfirmationRequest {
    pub fn respond(&self, allow: bool) {
        if let Some(sender) = self.responder.lock().ok().and_then(|mut slot| slot.take()) {
            let _ = sender.send(allow);
        }
    }
}

#[derive(Debug)]
pub enum AgentEvent {
    Working { step: usize, message: String },
    SetupRequired { message: String },
    AwaitingUser { question: String },
    AwaitingConfirmation(ConfirmationRequest),
    Reply(String),
    Error(String),
    Finished,
    Cancelled,
}
pub type EventSink = Arc<dyn Fn(AgentEvent) + Send + Sync>;

/// Cooperative cancellation: an in-flight portal notification is never
/// interrupted, but no subsequent decision or side effect is allowed.
#[derive(Clone, Debug, Default)]
pub struct RunControl(Arc<AtomicBool>);
impl RunControl {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub struct AgentServices {
    pub decision: Arc<dyn DecisionProvider>,
    pub vision: Arc<dyn VisionProvider>,
    pub text: Option<Arc<dyn TextProvider>>,
    pub apps: Arc<dyn AppCatalog>,
    pub policy: PolicyEngine,
}

impl AgentServices {
    pub fn from_config(config: &Config) -> Result<Self> {
        let client = Client::builder().build()?;
        let decision = Arc::new(JevClient::new(
            client.clone(),
            JevConfig {
                base_url: config.settings.jev.base_url.clone(),
                model: config.settings.jev.model.clone(),
                api_key: config.typesafe_api_key.clone(),
                timeout: std::time::Duration::from_secs(30),
            },
        ));
        let vision = Arc::new(VocaelaVisionProvider::new(
            client,
            config.vocaela_url.clone(),
        ));
        let text = config
            .settings
            .text_provider_config(config.text_provider_api_key.clone())
            .map(OpenAiCompatibleTextProvider::new)
            .transpose()?
            .map(|provider| Arc::new(provider) as Arc<dyn TextProvider>);
        Ok(Self {
            decision,
            vision,
            text,
            apps: Arc::new(GioAppCatalog::discover()),
            policy: PolicyEngine::new(config.settings.policy.clone()),
        })
    }
}

#[allow(dead_code)]
pub async fn run_goal(config: Config, goal: String, sink: EventSink) -> Result<()> {
    run_goal_cancelable(config, goal, sink, RunControl::default()).await
}

pub async fn run_goal_cancelable(
    config: Config,
    goal: String,
    sink: EventSink,
    control: RunControl,
) -> Result<()> {
    let services = AgentServices::from_config(&config)?;
    run_goal_with_services_control(config, goal, sink, services, control).await
}

#[allow(dead_code)]
pub async fn run_goal_with_services(
    config: Config,
    goal: String,
    sink: EventSink,
    services: AgentServices,
) -> Result<()> {
    run_goal_with_services_control(config, goal, sink, services, RunControl::default()).await
}

async fn run_goal_with_services_control(
    config: Config,
    goal: String,
    sink: EventSink,
    services: AgentServices,
    control: RunControl,
) -> Result<()> {
    if !config
        .typesafe_api_key
        .as_ref()
        .is_some_and(|key| !key.is_empty())
    {
        emit(
            &sink,
            AgentEvent::SetupRequired {
                message: "Decision engine needs setup.".into(),
            },
        );
        return Ok(());
    }
    // Vision is a capability, not a prerequisite for unrelated operations such
    // as launching a known application. A failed health check is represented in
    // the decision state; attempting a later VisualAction will fail safely.
    let vision_available = match services.vision.health_check().await {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "vision is not ready; visual actions are unavailable");
            false
        }
    };
    let debug = DebugWriter::new(&config.debug_dir, config.settings.debug.persist_screenshots)
        .context("create debug directory")?;
    let static_frame = config
        .image
        .as_ref()
        .map(|path| load_frame(path))
        .transpose()?;
    let portal = if static_frame.is_none() {
        Some(
            PortalSession::start()
                .await
                .context("start persistent Wayland portal session")?,
        )
    } else {
        None
    };
    let mut state = AgentState::new(
        goal,
        CapabilityAvailability {
            vision: vision_available,
            text_provider: services.text.is_some(),
            desktop_input: portal.is_some(),
            app_catalog: true,
        },
        DesktopState {
            live_capture: static_frame.is_none(),
            execute_enabled: config.execute,
        },
        services.apps.apps(),
    );
    let task_started = Instant::now();
    let mut machine = MachineState::Idle;
    for step in 1..=config.max_steps {
        if control.is_cancelled() {
            transition(&mut machine, MachineState::Cancelled);
            emit(&sink, AgentEvent::Cancelled);
            close_portal(&portal).await;
            return Ok(());
        }
        transition(&mut machine, MachineState::Decide);
        let step_started = Instant::now();
        emit(
            &sink,
            AgentEvent::Working {
                step,
                message: "Deciding next capability".into(),
            },
        );
        // Fan-out answers are independent. A selected capability can therefore
        // occasionally arrive with its required closed-set argument as
        // `__none__`. Do not synthesize an argument or perform input: add a
        // small, traceable context artifact and request one corrected decision.
        let (decision, action) = {
            let mut retry = 0;
            loop {
                let decision = match services.decision.decide(&state).await {
                    Ok(decision) => decision,
                    Err(error) => return fail(&portal, &sink, format!("{error:#}")).await,
                };
                match resolve(&decision, &state) {
                    Ok(action) => break (decision, action),
                    Err(error) if retry == 0 => {
                        retry += 1;
                        let message = format!(
                            "The previous Jev decision was incomplete and caused no input: {error}. Choose an available capability and provide every argument required by that capability."
                        );
                        let id = state.artifacts.insert(
                            ArtifactKind::StructuredContext,
                            ArtifactSource::System,
                            message,
                        );
                        tracing::warn!(step, artifact = %id.0, %error, "retrying incomplete Jev decision");
                    }
                    Err(error) => return fail(&portal, &sink, format!("{error:#}")).await,
                }
            }
        };
        debug
            .write_decision(
                step,
                &state,
                &decision.action.choice,
                decision.action.confidence,
                &decision.action.probabilities,
            )
            .context("write decision debug")?;
        if control.is_cancelled() {
            transition(&mut machine, MachineState::Cancelled);
            emit(&sink, AgentEvent::Cancelled);
            close_portal(&portal).await;
            return Ok(());
        }
        transition(&mut machine, MachineState::Resolve);
        let risk = decision.risk()?;
        let evidence = match DecisionEvidence::from_choice(
            &decision.action.choice,
            &decision.action.probabilities,
            decision.action.confidence,
        ) {
            Ok(evidence) => evidence,
            Err(error) => return fail(&portal, &sink, error).await,
        };
        tracing::info!(run_id = %state.run_id, step, capability = %decision.action.choice, selected_probability = evidence.selected_probability, runner_up_probability = evidence.runner_up_probability, action_margin = evidence.margin, confidence = evidence.confidence, risk = %risk.choice_id(), request_id = ?decision.request_id, model = ?decision.model, jev_ms = decision.latency_ms, "Jev decision");
        let policy_started = Instant::now();
        let evaluation = services.policy.evaluate(&action, &evidence, risk);
        let policy_ms = policy_started.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(step, effective_risk = %evaluation.effective_risk.choice_id(), policy_ms, decision = ?evaluation.decision, "policy evaluation");
        if config.execute {
            transition(&mut machine, MachineState::PolicyCheck);
            match &evaluation.decision {
                PolicyDecision::Reject(reason) => {
                    return fail(&portal, &sink, reason.clone()).await;
                }
                PolicyDecision::RequireConfirmation => {
                    transition(&mut machine, MachineState::AwaitingConfirmation);
                    let preview = preview(&state, &action);
                    let (sender, receiver) = oneshot::channel();
                    emit(
                        &sink,
                        AgentEvent::AwaitingConfirmation(ConfirmationRequest {
                            action: format!("{action:?}"),
                            preview,
                            responder: Arc::new(std::sync::Mutex::new(Some(sender))),
                        }),
                    );
                    match receiver.await {
                        Ok(true) => {
                            transition(&mut machine, MachineState::Execute);
                            tracing::info!(step, "confirmation accepted")
                        }
                        Ok(false) | Err(_) => {
                            transition(&mut machine, MachineState::Cancelled);
                            record(
                                &mut state,
                                step,
                                action,
                                ActionStatus::AwaitingConfirmation,
                                InputResult::NotApplicable,
                                VerificationResult::NotApplicable,
                                step_started,
                                Some("cancelled by user".into()),
                            );
                            emit(&sink, AgentEvent::Cancelled);
                            close_portal(&portal).await;
                            return Ok(());
                        }
                    }
                }
                PolicyDecision::Allow => {}
            }
        }
        match action {
            AgentAction::VisualAction { .. } => transition(&mut machine, MachineState::Observe),
            AgentAction::RequestText { .. } => {
                transition(&mut machine, MachineState::RequestingText)
            }
            _ => transition(&mut machine, MachineState::Execute),
        }
        let outcome = execute_action(
            step,
            &action,
            &mut state,
            &services,
            portal.as_ref(),
            static_frame.as_ref(),
            &debug,
            config.execute,
            config.action_settle_ms,
        )
        .await;
        match outcome {
            Ok((status, input, verification, error)) => {
                transition(&mut machine, MachineState::Execute);
                if let Some(message) = error.as_ref() {
                    match &action {
                        AgentAction::RequestText { .. } if services.text.is_none() => {
                            transition(&mut machine, MachineState::AwaitingTextProviderSetup);
                            emit(
                                &sink,
                                AgentEvent::SetupRequired {
                                    message: message.clone(),
                                },
                            );
                        }
                        AgentAction::AskUser { .. } => {
                            transition(&mut machine, MachineState::AwaitingUser);
                            emit(
                                &sink,
                                AgentEvent::AwaitingUser {
                                    question: message.clone(),
                                },
                            );
                        }
                        AgentAction::ReplyToUser { .. } => {
                            emit(&sink, AgentEvent::Reply(message.clone()))
                        }
                        _ => {}
                    }
                }
                if matches!(action, AgentAction::VisualAction { .. }) {
                    transition(&mut machine, MachineState::Verify);
                }
                record(
                    &mut state,
                    step,
                    action.clone(),
                    status,
                    input,
                    verification,
                    step_started,
                    error,
                );
                debug
                    .write_metrics(step, &state.recent_history)
                    .context("write step metrics")?;
                tracing::info!(
                    step,
                    total_step_ms = step_started.elapsed().as_secs_f64() * 1000.0,
                    total_task_ms = task_started.elapsed().as_secs_f64() * 1000.0,
                    "action recorded"
                );
                match action {
                    AgentAction::Done => {
                        transition(&mut machine, MachineState::Completed);
                        emit(&sink, AgentEvent::Finished);
                        close_portal(&portal).await;
                        return Ok(());
                    }
                    AgentAction::Abort => {
                        transition(&mut machine, MachineState::Failed);
                        return fail(&portal, &sink, "agent aborted".into()).await;
                    }
                    AgentAction::AskUser { .. } => {
                        close_portal(&portal).await;
                        return Ok(());
                    }
                    // A static image cannot reflect an input result. Treat one
                    // simulated visual action as a completed offline grounding
                    // probe instead of repeatedly asking vision about stale
                    // pixels until MAX_STEPS.
                    AgentAction::VisualAction {
                        instruction_artifact,
                    } if static_frame.is_some()
                        && !config.execute
                        && instruction_artifact == state.goal =>
                    {
                        transition(&mut machine, MachineState::Completed);
                        emit(&sink, AgentEvent::Finished);
                        return Ok(());
                    }
                    // Live dry-run deliberately never mutates the desktop, so
                    // a second visual iteration would only re-ground unchanged
                    // pixels. One simulated visual action validates the live
                    // capture/vision path and leaves semantic execution for an
                    // explicitly authorized --execute run.
                    AgentAction::VisualAction { .. }
                        if static_frame.is_none() && !config.execute =>
                    {
                        transition(&mut machine, MachineState::Completed);
                        emit(&sink, AgentEvent::Finished);
                        close_portal(&portal).await;
                        return Ok(());
                    }
                    _ => {}
                }
            }
            Err(error) => {
                transition(&mut machine, MachineState::Failed);
                record(
                    &mut state,
                    step,
                    action,
                    ActionStatus::Failed,
                    InputResult::Failed,
                    VerificationResult::Unknown,
                    step_started,
                    Some(format!("{error:#}")),
                );
                debug
                    .write_metrics(step, &state.recent_history)
                    .context("write failed step metrics")?;
                return fail(&portal, &sink, format!("{error:#}")).await;
            }
        }
    }
    tracing::debug!(state = ?machine, "agent loop reached its step limit");
    fail(
        &portal,
        &sink,
        format!("maximum step count ({}) reached", config.max_steps),
    )
    .await
}

async fn execute_action(
    step: usize,
    action: &AgentAction,
    state: &mut AgentState,
    services: &AgentServices,
    portal: Option<&PortalSession>,
    static_frame: Option<&Frame>,
    debug: &DebugWriter,
    execute: bool,
    settle_ms: u64,
) -> Result<(
    ActionStatus,
    InputResult,
    VerificationResult,
    Option<String>,
)> {
    match action {
        AgentAction::LaunchApp { app_id } => {
            if execute {
                services.apps.launch(app_id)?;
            }
            Ok((
                if execute {
                    ActionStatus::Executed
                } else {
                    ActionStatus::Simulated
                },
                if execute {
                    InputResult::Delivered
                } else {
                    InputResult::Simulated
                },
                VerificationResult::Unknown,
                None,
            ))
        }
        AgentAction::VisualAction {
            instruction_artifact,
        } => {
            if !state.available_capabilities.vision {
                anyhow::bail!("vision is not ready; cannot perform a visual action")
            }
            let instruction = state
                .artifacts
                .get(instruction_artifact)
                .context("visual artifact disappeared")?
                .value
                .clone();
            let capture_started = Instant::now();
            let before = current_frame(portal, static_frame)?;
            let capture_ms = capture_started.elapsed().as_secs_f64() * 1000.0;
            let grounding = services
                .vision
                .ground(&before, &instruction)
                .await
                .context("visual grounding failed")?;
            match grounding {
                GroundingResult::NotFound { raw_response } => {
                    tracing::info!(step, instruction, response = %raw_response, "vision target not found");
                    Ok((
                        ActionStatus::Failed,
                        InputResult::NotApplicable,
                        VerificationResult::Unknown,
                        Some("vision target not found".into()),
                    ))
                }
                GroundingResult::Found(mut found) => {
                    found.frame_acquire_ms = capture_ms;
                    let (pixel_x, pixel_y) = before.normalized_to_pixel(found.bbox);
                    tracing::info!(
                        step,
                        instruction,
                        capture_ms,
                        image_encode_ms = found.image_encode_ms,
                        base64_ms = found.base64_ms,
                        vocaela_http_ms = found.vocaela_http_ms,
                        vision_parse_ms = found.vision_parse_ms,
                        grounding_total_ms = found.grounding_total_ms,
                        normalized_x = found.bbox.center_x,
                        normalized_y = found.bbox.center_y,
                        pixel_x,
                        pixel_y,
                        "vision grounding"
                    );
                    tracing::debug!(step, response = %found.raw_response, "vision raw response");
                    debug
                        .write_step(
                            step,
                            &before,
                            Some(found.bbox),
                            &instruction,
                            &state.recent_history,
                            Some(&found.png),
                        )
                        .context("write visual debug")?;
                    if !execute {
                        return Ok((
                            ActionStatus::Simulated,
                            InputResult::Simulated,
                            VerificationResult::NotApplicable,
                            None,
                        ));
                    }
                    let input_started = Instant::now();
                    portal
                        .context("live portal is unavailable")?
                        .click(&before, found.bbox)
                        .await
                        .context("portal click failed")?;
                    let input_ms = input_started.elapsed().as_secs_f64() * 1000.0;
                    tokio::time::sleep(Duration::from_millis(settle_ms)).await;
                    let after = current_frame(portal, static_frame)?;
                    let verification_started = Instant::now();
                    let verification = screen_delta(&before, &after);
                    tracing::info!(step, input_ms, verification_ms = verification_started.elapsed().as_secs_f64() * 1000.0, verification = ?verification, "visual input verification");
                    let status = if verification == VerificationResult::NoVisibleChange {
                        ActionStatus::NoVisibleChange
                    } else {
                        ActionStatus::Executed
                    };
                    Ok((status, InputResult::Delivered, verification, None))
                }
            }
        }
        AgentAction::TypeArtifact { artifact } => {
            let text = state
                .artifacts
                .get(artifact)
                .context("type artifact disappeared")?
                .value
                .clone();
            if execute {
                let started = Instant::now();
                portal
                    .context("live portal is unavailable")?
                    .input_text(&text)
                    .await
                    .context("portal text input failed")?;
                tracing::info!(step, input_ms = started.elapsed().as_secs_f64() * 1000.0, artifact = %artifact.0, "text input");
                tokio::time::sleep(Duration::from_millis(settle_ms)).await;
                Ok((
                    ActionStatus::Executed,
                    InputResult::Delivered,
                    VerificationResult::Unknown,
                    None,
                ))
            } else {
                Ok((
                    ActionStatus::Simulated,
                    InputResult::Simulated,
                    VerificationResult::NotApplicable,
                    None,
                ))
            }
        }
        AgentAction::PressKey { key } => {
            if execute {
                portal
                    .context("live portal is unavailable")?
                    .press_key(key.clone())
                    .await?;
                tokio::time::sleep(Duration::from_millis(settle_ms)).await;
                Ok((
                    ActionStatus::Executed,
                    InputResult::Delivered,
                    VerificationResult::Unknown,
                    None,
                ))
            } else {
                Ok((
                    ActionStatus::Simulated,
                    InputResult::Simulated,
                    VerificationResult::NotApplicable,
                    None,
                ))
            }
        }
        AgentAction::Hotkey { keys } => {
            if execute {
                portal
                    .context("live portal is unavailable")?
                    .hotkey(keys)
                    .await?;
                tokio::time::sleep(Duration::from_millis(settle_ms)).await;
                Ok((
                    ActionStatus::Executed,
                    InputResult::Delivered,
                    VerificationResult::Unknown,
                    None,
                ))
            } else {
                Ok((
                    ActionStatus::Simulated,
                    InputResult::Simulated,
                    VerificationResult::NotApplicable,
                    None,
                ))
            }
        }
        AgentAction::Scroll { direction } => {
            if execute {
                portal
                    .context("live portal is unavailable")?
                    .scroll(direction.clone())
                    .await?;
                tokio::time::sleep(Duration::from_millis(settle_ms)).await;
                Ok((
                    ActionStatus::Executed,
                    InputResult::Delivered,
                    VerificationResult::Unknown,
                    None,
                ))
            } else {
                Ok((
                    ActionStatus::Simulated,
                    InputResult::Simulated,
                    VerificationResult::NotApplicable,
                    None,
                ))
            }
        }
        AgentAction::RequestText { kind } => {
            let Some(provider) = &services.text else {
                return Ok((
                    ActionStatus::AwaitingUser,
                    InputResult::NotApplicable,
                    VerificationResult::NotApplicable,
                    Some("This task needs text generation. Configure text provider.".into()),
                ));
            };
            let goal = state
                .artifacts
                .get(&state.goal)
                .map(|artifact| artifact.value.clone())
                .unwrap_or_default();
            let generation = provider
                .generate(TextRequest {
                    kind: kind.clone(),
                    goal,
                    context: state
                        .recent_history
                        .iter()
                        .map(|record| format!("{:?}", record.selected_action))
                        .collect(),
                    requirements: vec![],
                })
                .await?;
            let artifact_kind = match kind {
                crate::action::TextRequestKind::ComposeText
                | crate::action::TextRequestKind::ExtractTextToType
                | crate::action::TextRequestKind::TransformText => ArtifactKind::TextToType,
                crate::action::TextRequestKind::VisualInstruction => {
                    ArtifactKind::VisualInstruction
                }
                crate::action::TextRequestKind::ReplyToUser => ArtifactKind::AssistantReply,
                crate::action::TextRequestKind::AskClarification => {
                    ArtifactKind::ClarificationQuestion
                }
            };
            let id = state.artifacts.insert(
                artifact_kind,
                ArtifactSource::TextProvider,
                generation.text,
            );
            tracing::info!(step, artifact = %id.0, text_provider_ms = generation.latency_ms, "text artifact created");
            Ok((
                ActionStatus::Executed,
                InputResult::NotApplicable,
                VerificationResult::NotApplicable,
                None,
            ))
        }
        AgentAction::AskUser { artifact } => {
            let question = state
                .artifacts
                .get(artifact)
                .context("question artifact disappeared")?
                .value
                .clone();
            Ok((
                ActionStatus::AwaitingUser,
                InputResult::NotApplicable,
                VerificationResult::NotApplicable,
                Some(question),
            ))
        }
        AgentAction::ReplyToUser { artifact } => {
            let message = state
                .artifacts
                .get(artifact)
                .context("reply artifact disappeared")?
                .value
                .clone();
            Ok((
                ActionStatus::Executed,
                InputResult::NotApplicable,
                VerificationResult::NotApplicable,
                Some(message),
            ))
        }
        AgentAction::Wait { duration_ms } => {
            tokio::time::sleep(Duration::from_millis(*duration_ms)).await;
            Ok((
                ActionStatus::Executed,
                InputResult::NotApplicable,
                VerificationResult::Unknown,
                None,
            ))
        }
        AgentAction::Done => Ok((
            ActionStatus::Executed,
            InputResult::NotApplicable,
            VerificationResult::NotApplicable,
            None,
        )),
        AgentAction::Abort => Ok((
            ActionStatus::Failed,
            InputResult::NotApplicable,
            VerificationResult::NotApplicable,
            Some("agent aborted".into()),
        )),
    }
}

fn current_frame(portal: Option<&PortalSession>, static_frame: Option<&Frame>) -> Result<Frame> {
    static_frame
        .cloned()
        .or_else(|| portal.and_then(|session| session.capture_latest().ok()))
        .context("no current frame is available")
}
fn screen_delta(before: &Frame, after: &Frame) -> VerificationResult {
    if before.pixel_width != after.pixel_width || before.pixel_height != after.pixel_height {
        return VerificationResult::ScreenChanged;
    }
    let samples = before.rgba.len().min(after.rgba.len()).min(64 * 64 * 4);
    if samples == 0 {
        return VerificationResult::Unknown;
    }
    let stride = (before.rgba.len() / samples).max(1);
    let difference: f64 = (0..samples)
        .map(|index| before.rgba[index * stride] as f64 - after.rgba[index * stride] as f64)
        .map(f64::abs)
        .sum::<f64>()
        / samples as f64;
    if difference > 4.0 {
        VerificationResult::ScreenChanged
    } else {
        VerificationResult::NoVisibleChange
    }
}
fn preview(state: &AgentState, action: &AgentAction) -> Option<String> {
    match action {
        AgentAction::TypeArtifact { artifact }
        | AgentAction::AskUser { artifact }
        | AgentAction::ReplyToUser { artifact }
        | AgentAction::VisualAction {
            instruction_artifact: artifact,
        } => state
            .artifacts
            .get(artifact)
            .map(|artifact| artifact.value.clone()),
        _ => None,
    }
}
fn record(
    state: &mut AgentState,
    step: usize,
    action: AgentAction,
    status: ActionStatus,
    input: InputResult,
    verification: VerificationResult,
    started: Instant,
    error: Option<String>,
) {
    if let Some(message) = error
        .as_ref()
        .filter(|_| matches!(action, AgentAction::AskUser { .. }))
    {
        tracing::info!(question = %message, "awaiting user");
    }
    state.push_record(ActionRecord {
        step,
        selected_action: action,
        status,
        input_result: input,
        verification,
        started_at_ms: now_ms(),
        duration_ms: started.elapsed().as_secs_f64() * 1000.0,
        error,
    });
}
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn transition(current: &mut MachineState, next: MachineState) {
    tracing::debug!(from = ?current, to = ?next, "agent state transition");
    *current = next;
}
async fn close_portal(portal: &Option<PortalSession>) {
    if let Some(portal) = portal {
        if let Err(error) = portal.close().await {
            tracing::warn!(%error, "failed to close portal session");
        }
    }
}
async fn fail<T>(portal: &Option<PortalSession>, sink: &EventSink, message: String) -> Result<T> {
    tracing::error!(%message, "agent run failed");
    emit(sink, AgentEvent::Error(message.clone()));
    close_portal(portal).await;
    anyhow::bail!(message)
}
fn emit(sink: &EventSink, event: AgentEvent) {
    sink(event);
}
fn load_frame(path: &Path) -> Result<Frame> {
    Ok(Frame::from_image(image::open(path).with_context(|| {
        format!("read image {}", path.display())
    })?))
}
pub fn run_gate() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        action::{AppId, AppInfo, ArtifactKind, TextRequestKind},
        app_catalog::{AppCatalog, AppCatalogError},
        jev::{ChoiceAnswer, JevDecision},
        settings::Settings,
        text_provider::{TextGeneration, TextProviderError},
        vision::{BoundingBox, FoundGrounding, VisionError},
    };
    use async_trait::async_trait;
    use std::collections::BTreeMap;
    #[test]
    fn screen_delta_distinguishes_change() {
        let mut before = Frame {
            rgba: vec![0; 400],
            pixel_width: 10,
            pixel_height: 10,
            logical_width: 10,
            logical_height: 10,
            stream_node_id: 0,
        };
        let unchanged = before.clone();
        assert_eq!(
            screen_delta(&before, &unchanged),
            VerificationResult::NoVisibleChange
        );
        before.rgba.fill(255);
        assert_eq!(
            screen_delta(&before, &unchanged),
            VerificationResult::ScreenChanged
        );
    }

    #[derive(Default)]
    struct MockDecision;
    fn answer(choice: impl Into<String>) -> ChoiceAnswer {
        let choice = choice.into();
        ChoiceAnswer {
            probabilities: BTreeMap::from([(choice.clone(), 0.99)]),
            choice,
            confidence: 0.99,
        }
    }
    fn blank_decision(action: impl Into<String>) -> JevDecision {
        JevDecision {
            action: answer(action),
            application: answer("__none__"),
            visual_artifact: answer("__none__"),
            type_artifact: answer("__none__"),
            user_message_artifact: answer("__none__"),
            key: answer("__none__"),
            scroll_direction: answer("__none__"),
            wait_duration: answer("__none__"),
            text_request_kind: answer("__none__"),
            risk_level: answer("low"),
            request_id: None,
            latency_ms: 1.0,
            model: Some("mock-je v".into()),
        }
    }
    #[async_trait]
    impl DecisionProvider for MockDecision {
        async fn decide(
            &self,
            state: &AgentState,
        ) -> Result<JevDecision, crate::jev::DecisionError> {
            let mut decision =
                if !state
                    .recent_history
                    .iter()
                    .any(|record| matches!(record.selected_action, AgentAction::LaunchApp { .. }))
                {
                    let mut decision = blank_decision("launch_app");
                    decision.application = answer("example-chat.desktop");
                    decision
                } else if state
                    .artifacts
                    .by_kind(ArtifactKind::VisualInstruction)
                    .is_empty()
                {
                    let mut decision = blank_decision("request_text");
                    decision.text_request_kind = answer("visual_instruction");
                    decision
                } else if !state.recent_history.iter().any(|record| {
                    matches!(record.selected_action, AgentAction::VisualAction { .. })
                }) {
                    let mut decision = blank_decision("visual_action");
                    decision.visual_artifact = answer(
                        state.artifacts.by_kind(ArtifactKind::VisualInstruction)[0]
                            .id
                            .0
                            .clone(),
                    );
                    decision
                } else if state.artifacts.by_kind(ArtifactKind::TextToType).is_empty() {
                    let mut decision = blank_decision("request_text");
                    decision.text_request_kind = answer("compose_text");
                    decision
                } else if !state.recent_history.iter().any(|record| {
                    matches!(record.selected_action, AgentAction::TypeArtifact { .. })
                }) {
                    let mut decision = blank_decision("type_artifact");
                    decision.type_artifact = answer(
                        state.artifacts.by_kind(ArtifactKind::TextToType)[0]
                            .id
                            .0
                            .clone(),
                    );
                    decision
                } else if !state
                    .recent_history
                    .iter()
                    .any(|record| matches!(record.selected_action, AgentAction::PressKey { .. }))
                {
                    let mut decision = blank_decision("press_key");
                    decision.key = answer("enter");
                    decision
                } else {
                    blank_decision("done")
                };
            decision.risk_level = answer("low");
            Ok(decision)
        }
        async fn test_connection(&self) -> Result<(), crate::jev::DecisionError> {
            Ok(())
        }
    }
    struct MockText(Arc<std::sync::Mutex<Vec<TextRequestKind>>>);
    #[async_trait]
    impl TextProvider for MockText {
        async fn generate(
            &self,
            request: TextRequest,
        ) -> Result<TextGeneration, TextProviderError> {
            self.0.lock().unwrap().push(request.kind.clone());
            Ok(TextGeneration {
                text: match request.kind {
                    TextRequestKind::VisualInstruction => "Open the conversation with Alex".into(),
                    _ => "Hello Alex, I will be late.".into(),
                },
                latency_ms: 1.0,
            })
        }
        async fn test_connection(&self) -> Result<(), TextProviderError> {
            Ok(())
        }
    }
    struct MockVision(Arc<std::sync::Mutex<Vec<String>>>);
    #[async_trait]
    impl VisionProvider for MockVision {
        async fn health_check(&self) -> Result<(), VisionError> {
            Ok(())
        }
        async fn ground(
            &self,
            _frame: &Frame,
            instruction: &str,
        ) -> Result<GroundingResult, VisionError> {
            self.0.lock().unwrap().push(instruction.into());
            Ok(GroundingResult::Found(FoundGrounding {
                bbox: BoundingBox {
                    center_x: 0.5,
                    center_y: 0.5,
                    width: 0.1,
                    height: 0.1,
                },
                raw_response: "[0.5,0.5,0.1,0.1]".into(),
                frame_acquire_ms: 0.0,
                image_encode_ms: 0.0,
                base64_ms: 0.0,
                vocaela_http_ms: 0.0,
                vision_parse_ms: 0.0,
                grounding_total_ms: 0.0,
                png: vec![],
            }))
        }
    }
    struct MockApps;
    impl AppCatalog for MockApps {
        fn apps(&self) -> Vec<AppInfo> {
            vec![AppInfo {
                id: AppId("example-chat.desktop".into()),
                display_name: "Example Chat".into(),
                description: None,
            }]
        }
        fn launch(&self, _id: &AppId) -> Result<(), AppCatalogError> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn generic_controller_uses_artifacts_for_a_fictitious_app() {
        let root = std::env::temp_dir().join(format!("vocs-controller-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let image_path = root.join("frame.png");
        image::RgbaImage::from_pixel(10, 10, image::Rgba([0, 0, 0, 255]))
            .save(&image_path)
            .unwrap();
        let text_requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let visual_requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let config = Config {
            settings: Settings::default(),
            typesafe_api_key: Some("mock".into()),
            text_provider_api_key: None,
            vocaela_url: "http://unused".into(),
            max_steps: 8,
            action_settle_ms: 0,
            execute: false,
            goal: None,
            image: Some(image_path),
            debug_dir: root.join("runs"),
        };
        let services = AgentServices {
            decision: Arc::new(MockDecision),
            vision: Arc::new(MockVision(visual_requests.clone())),
            text: Some(Arc::new(MockText(text_requests.clone()))),
            apps: Arc::new(MockApps),
            policy: PolicyEngine::new(crate::policy::PolicyConfig::default()),
        };
        let sink: EventSink = Arc::new(|_| {});
        run_goal_with_services(
            config,
            "Open Example Chat and send a polite message to Alex saying I am late.".into(),
            sink,
            services,
        )
        .await
        .unwrap();
        assert_eq!(
            text_requests.lock().unwrap().as_slice(),
            &[
                TextRequestKind::VisualInstruction,
                TextRequestKind::ComposeText
            ]
        );
        assert_eq!(
            visual_requests.lock().unwrap().as_slice(),
            &["Open the conversation with Alex"]
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

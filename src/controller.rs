use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use reqwest::Client;
use tokio::sync::{Semaphore, mpsc};

use crate::{
    action::{
        ActionRecord, ActionStatus, AgentAction, AgentState, ArtifactKind, ArtifactSource,
        CapabilityAvailability, DesktopState, FrameObservation, InputResult, ReplanReason,
        TaskGuidance, VerificationResult,
    },
    app_catalog::{AppCatalog, GioAppCatalog},
    config::Config,
    debug::DebugWriter,
    desktop::{Frame, PortalSession, TextInputBackend},
    jev::{DecisionProvider, JevClient, JevConfig, build_candidates, resolve},
    policy::{PolicyDecision, PolicyEngine},
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
pub enum AgentEvent {
    Working { step: usize, message: String },
    SetupRequired { message: String },
    AwaitingUser { question: String },
    AwaitingConfirmation { preview: String },
    Reply(String),
    Error(String),
    Finished,
    Cancelled,
}
pub type EventSink = Arc<dyn Fn(AgentEvent) + Send + Sync>;

/// Cooperative cancellation: an in-flight portal notification is never
/// interrupted, but no subsequent decision or side effect is allowed.
enum AgentCommand {
    UserReply(String),
    AllowCommit,
    CancelCommit,
    ProvidersUpdated(Option<Arc<dyn TextProvider>>),
}

/// Cooperative control of one persistent run. Commands are intentionally
/// narrow: they resume an already-paused state, never create a concurrent run.
#[derive(Clone)]
pub struct RunControl {
    cancelled: Arc<AtomicBool>,
    awaiting_user: Arc<AtomicBool>,
    awaiting_confirmation: Arc<AtomicBool>,
    awaiting_text_setup: Arc<AtomicBool>,
    commands: mpsc::UnboundedSender<AgentCommand>,
    receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<AgentCommand>>>>,
}

impl Default for RunControl {
    fn default() -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            awaiting_user: Arc::new(AtomicBool::new(false)),
            awaiting_confirmation: Arc::new(AtomicBool::new(false)),
            awaiting_text_setup: Arc::new(AtomicBool::new(false)),
            commands,
            receiver: Arc::new(Mutex::new(Some(receiver))),
        }
    }
}

impl RunControl {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn is_awaiting_user(&self) -> bool {
        self.awaiting_user.load(Ordering::Acquire)
    }
    pub fn is_awaiting_text_setup(&self) -> bool {
        self.awaiting_text_setup.load(Ordering::Acquire)
    }
    pub fn allow_commit(&self) {
        let _ = self.commands.send(AgentCommand::AllowCommit);
    }
    pub fn cancel_commit(&self) {
        let _ = self.commands.send(AgentCommand::CancelCommit);
    }
    pub fn submit_user_reply(&self, text: String) {
        let _ = self.commands.send(AgentCommand::UserReply(text));
    }
    pub fn providers_updated(&self, text_provider: Option<Arc<dyn TextProvider>>) {
        let _ = self
            .commands
            .send(AgentCommand::ProvidersUpdated(text_provider));
    }
    fn take_receiver(&self) -> Result<mpsc::UnboundedReceiver<AgentCommand>> {
        self.receiver
            .lock()
            .map_err(|_| anyhow::anyhow!("agent command receiver poisoned"))?
            .take()
            .context("agent run was already started")
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
    mut services: AgentServices,
    control: RunControl,
) -> Result<()> {
    let mut commands = control.take_receiver()?;
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
    // PipeWire remains live in the background. A frame is required only by a
    // visual action or an explicit verification, never merely to decide.
    let mut state = AgentState::new(
        goal,
        CapabilityAvailability {
            vision: vision_available,
            text_provider: services.text.is_some(),
            desktop_input: portal.is_some(),
            text_input: portal
                .as_ref()
                .is_some_and(PortalSession::supports_text_input),
            app_catalog: true,
        },
        DesktopState {
            live_capture: static_frame.is_none(),
            execute_enabled: config.execute,
            last_grounded_frame: None,
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
        if state.guidance.is_some()
            && state.replan_reason.is_none()
            && !state.terminal_evidence
            && !build_candidates(&state).has_actionable()
        {
            state.set_replan(crate::action::ReplanReason::GuidanceExhausted);
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
        let decision = match services.decision.decide(&state).await {
            Ok(decision) => decision,
            Err(error) => return fail(&portal, &sink, format!("{error:#}")).await,
        };
        let resolved = match resolve(&decision) {
            Ok(action) => action,
            Err(error) => return fail(&portal, &sink, format!("{error:#}")).await,
        };
        debug
            .write_decision(
                step,
                &state,
                &decision.action.choice,
                decision.action.confidence,
                &decision.action.probabilities,
                &decision.candidates,
            )
            .context("write decision debug")?;
        if control.is_cancelled() {
            transition(&mut machine, MachineState::Cancelled);
            emit(&sink, AgentEvent::Cancelled);
            close_portal(&portal).await;
            return Ok(());
        }
        transition(&mut machine, MachineState::Resolve);
        tracing::info!(run_id = %state.run_id, step, capability = %decision.action.choice, resolved_action = ?resolved.action, hint_id = ?resolved.hint_id, probabilities = ?decision.action.probabilities, selected_probability = ?decision.action.selected_probability(), runner_up_probability = ?decision.action.runner_up_probability(), action_margin = ?decision.action.margin(), confidence = decision.action.confidence, request_id = ?decision.request_id, model = ?decision.model, state_revision = state.state_revision, observation_revision = state.observation_revision, guidance_revision = state.guidance_revision, replan_reason = ?state.replan_reason, jev_ms = decision.latency_ms, "Jev decision resolved");
        let policy_started = Instant::now();
        let evaluation = services.policy.evaluate(&resolved.action);
        let policy_ms = policy_started.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(step, policy_ms, decision = ?evaluation, "policy evaluation");
        if config.execute {
            transition(&mut machine, MachineState::PolicyCheck);
            match evaluation {
                PolicyDecision::Allow => {}
                PolicyDecision::RequireConfirmation { preview_artifact } => {
                    let preview = state
                        .artifacts
                        .get(&preview_artifact)
                        .map(|artifact| artifact.value.clone())
                        .unwrap_or_else(|| "Commit the requested operation".into());
                    transition(&mut machine, MachineState::AwaitingConfirmation);
                    control.awaiting_confirmation.store(true, Ordering::Release);
                    emit(&sink, AgentEvent::AwaitingConfirmation { preview });
                    let command = tokio::select! {
                        _ = wait_for_cancel(&control) => None,
                        command = commands.recv() => command,
                    };
                    control
                        .awaiting_confirmation
                        .store(false, Ordering::Release);
                    match command {
                        Some(AgentCommand::AllowCommit) => {
                            tracing::info!(step, "commit confirmation accepted");
                        }
                        Some(AgentCommand::CancelCommit) | None => {
                            transition(&mut machine, MachineState::Cancelled);
                            emit(&sink, AgentEvent::Cancelled);
                            close_portal(&portal).await;
                            return Ok(());
                        }
                        Some(AgentCommand::UserReply(_))
                        | Some(AgentCommand::ProvidersUpdated(_)) => {
                            return fail(
                                &portal,
                                &sink,
                                "commit confirmation requires Allow once or Cancel".into(),
                            )
                            .await;
                        }
                    }
                }
            }
        }
        match &resolved.action {
            AgentAction::VisualAction { .. } => transition(&mut machine, MachineState::Observe),
            AgentAction::RequestText { .. }
            | AgentAction::RequestPlan
            | AgentAction::RequestReplan => transition(&mut machine, MachineState::RequestingText),
            _ => transition(&mut machine, MachineState::Execute),
        }
        let outcome = execute_action(
            step,
            &resolved.action,
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
                let needs_text_setup = error.is_some()
                    && services.text.is_none()
                    && matches!(
                        resolved.action,
                        AgentAction::RequestText { .. }
                            | AgentAction::RequestPlan
                            | AgentAction::RequestReplan
                    );
                if let Some(message) = error.as_ref() {
                    match &resolved.action {
                        AgentAction::RequestText { .. }
                        | AgentAction::RequestPlan
                        | AgentAction::RequestReplan
                            if services.text.is_none() =>
                        {
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
                if matches!(resolved.action, AgentAction::VisualAction { .. }) {
                    transition(&mut machine, MachineState::Verify);
                }
                record(
                    &mut state,
                    step,
                    resolved.action.clone(),
                    resolved.hint_id.clone(),
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
                if needs_text_setup {
                    control.awaiting_text_setup.store(true, Ordering::Release);
                    let command = tokio::select! {
                        _ = wait_for_cancel(&control) => None,
                        command = commands.recv() => command,
                    };
                    control.awaiting_text_setup.store(false, Ordering::Release);
                    match command {
                        Some(AgentCommand::ProvidersUpdated(Some(provider))) => {
                            services.text = Some(provider);
                            state.available_capabilities.text_provider = true;
                            state.state_revision += 1;
                            transition(&mut machine, MachineState::Decide);
                            continue;
                        }
                        Some(AgentCommand::CancelCommit) | None => {
                            transition(&mut machine, MachineState::Cancelled);
                            emit(&sink, AgentEvent::Cancelled);
                            close_portal(&portal).await;
                            return Ok(());
                        }
                        Some(AgentCommand::ProvidersUpdated(None))
                        | Some(AgentCommand::AllowCommit)
                        | Some(AgentCommand::UserReply(_)) => {
                            return fail(
                                &portal,
                                &sink,
                                "text provider setup is still incomplete".into(),
                            )
                            .await;
                        }
                    }
                }
                match resolved.action {
                    AgentAction::Done => {
                        state.mark_completed();
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
                        control.awaiting_user.store(true, Ordering::Release);
                        let command = tokio::select! {
                            _ = wait_for_cancel(&control) => None,
                            command = commands.recv() => command,
                        };
                        control.awaiting_user.store(false, Ordering::Release);
                        match command {
                            Some(AgentCommand::UserReply(reply)) if !reply.trim().is_empty() => {
                                state.insert_artifact(
                                    ArtifactKind::UserReply,
                                    ArtifactSource::User,
                                    reply,
                                );
                                state.set_replan(ReplanReason::UserProvidedInformation);
                                continue;
                            }
                            Some(AgentCommand::CancelCommit) | None => {
                                transition(&mut machine, MachineState::Cancelled);
                                emit(&sink, AgentEvent::Cancelled);
                                close_portal(&portal).await;
                                return Ok(());
                            }
                            Some(AgentCommand::AllowCommit)
                            | Some(AgentCommand::ProvidersUpdated(_)) => {
                                return fail(
                                    &portal,
                                    &sink,
                                    "task is awaiting a user reply".into(),
                                )
                                .await;
                            }
                            Some(AgentCommand::UserReply(_)) => {
                                return fail(&portal, &sink, "user reply cannot be empty".into())
                                    .await;
                            }
                        }
                    }
                    // A static image cannot reflect an input result. Treat one
                    // simulated visual action as a completed offline grounding
                    // probe instead of repeatedly asking vision about stale
                    // pixels until MAX_STEPS.
                    AgentAction::VisualAction { .. }
                        if static_frame.is_some() && !config.execute =>
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
                    resolved.action,
                    resolved.hint_id,
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
            // A frame alone is not semantic observation. The revision changes
            // only after the grounder returned an observation about it.
            state.note_visual_observation();
            state.desktop_state.last_grounded_frame = Some(FrameObservation {
                pixel_width: before.pixel_width,
                pixel_height: before.pixel_height,
                logical_width: before.logical_width,
                logical_height: before.logical_height,
            });
            match grounding {
                GroundingResult::NotFound { raw_response } => {
                    tracing::info!(step, instruction, response = %raw_response, "vision target not found");
                    Ok((
                        ActionStatus::Failed,
                        InputResult::NotApplicable,
                        VerificationResult::GroundingNotFound,
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
                let before = current_frame(portal, static_frame).ok();
                let started = Instant::now();
                portal
                    .context("live portal is unavailable")?
                    .input_text(&text)
                    .await
                    .context("portal text input failed")?;
                tracing::info!(step, input_ms = started.elapsed().as_secs_f64() * 1000.0, artifact = %artifact.0, "text input");
                let verification = verify_input(portal, static_frame, before, settle_ms).await;
                Ok((
                    ActionStatus::Executed,
                    InputResult::Delivered,
                    verification,
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
                let before = current_frame(portal, static_frame).ok();
                let started = Instant::now();
                portal
                    .context("live portal is unavailable")?
                    .press_key(key.clone())
                    .await?;
                let verification = verify_input(portal, static_frame, before, settle_ms).await;
                tracing::info!(step, input_ms = started.elapsed().as_secs_f64() * 1000.0, verification = ?verification, "key input");
                Ok((
                    ActionStatus::Executed,
                    InputResult::Delivered,
                    verification,
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
        AgentAction::Hotkey { shortcut } => {
            if execute {
                let before = current_frame(portal, static_frame).ok();
                let started = Instant::now();
                portal
                    .context("live portal is unavailable")?
                    .hotkey(&shortcut.keys())
                    .await?;
                let verification = verify_input(portal, static_frame, before, settle_ms).await;
                tracing::info!(step, input_ms = started.elapsed().as_secs_f64() * 1000.0, verification = ?verification, "hotkey input");
                Ok((
                    ActionStatus::Executed,
                    InputResult::Delivered,
                    verification,
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
                let before = current_frame(portal, static_frame).ok();
                let started = Instant::now();
                portal
                    .context("live portal is unavailable")?
                    .scroll(direction.clone())
                    .await?;
                let verification = verify_input(portal, static_frame, before, settle_ms).await;
                tracing::info!(step, input_ms = started.elapsed().as_secs_f64() * 1000.0, verification = ?verification, "scroll input");
                Ok((
                    ActionStatus::Executed,
                    InputResult::Delivered,
                    verification,
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
        AgentAction::RequestText { artifact, kind } => {
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
            let (purpose, requirements) = state
                .guidance
                .as_ref()
                .and_then(|runtime| {
                    runtime
                        .guidance
                        .artifacts
                        .iter()
                        .find(|spec| &spec.id == artifact)
                })
                .and_then(|spec| match &spec.content {
                    crate::action::GuidanceArtifactContent::Generate {
                        purpose,
                        requirements,
                        ..
                    } => Some((purpose.clone(), requirements.clone())),
                    _ => None,
                })
                .context("pending text artifact disappeared from guidance")?;
            let generation = provider
                .generate(TextRequest {
                    kind: kind.clone(),
                    goal,
                    context: state
                        .recent_history
                        .iter()
                        .map(|record| format!("{:?}", record.selected_action))
                        .chain(std::iter::once(format!("Purpose: {purpose}")))
                        .collect(),
                    requirements,
                })
                .await?;
            let id = state
                .materialize_generated(artifact, generation.text)
                .map_err(anyhow::Error::msg)?;
            tracing::info!(step, artifact = %id.0, text_provider_ms = generation.latency_ms, "text artifact created");
            Ok((
                ActionStatus::Executed,
                InputResult::NotApplicable,
                VerificationResult::NotApplicable,
                None,
            ))
        }
        AgentAction::RequestPlan | AgentAction::RequestReplan => {
            let Some(provider) = &services.text else {
                return Ok((
                    ActionStatus::AwaitingUser,
                    InputResult::NotApplicable,
                    VerificationResult::NotApplicable,
                    Some("This task needs text generation to create task guidance. Configure text provider.".into()),
                ));
            };
            let goal = state
                .artifacts
                .get(&state.goal)
                .map(|artifact| artifact.value.clone())
                .unwrap_or_default();
            let generation = provider
                .generate(TextRequest {
                    kind: crate::action::TextRequestKind::TaskGuidance,
                    goal,
                    context: state
                        .recent_history
                        .iter()
                        .map(|record| format!("{:?}", record.selected_action))
                        .collect(),
                    requirements: vec![
                        "Return exactly one TaskGuidance JSON object with objective, constraints, artifacts, and hints.".into(),
                        "Artifacts are bounded ids with kinds visual_instruction, text_to_type, assistant_reply, clarification_question, or action_preview. Each content is either literal text or a generate request with a compatible text-request kind and purpose.".into(),
                        "Hints are unordered, non-authoritative candidates. Use only launch_app, visual_action, type_artifact, press_key, hotkey, scroll, ask_user, reply_to_user, wait, or commit. Never include a plan cursor, next step, coordinates, app names, request_plan, request_replan, done, or abort.".into(),
                        "Try a current-screen visual instruction before launch_app unless the user explicitly asks to launch an application or history contains GroundingNotFound.".into(),
                    ],
            })
            .await?;
            let guidance = TaskGuidance::parse(&generation.text).map_err(anyhow::Error::msg)?;
            state.set_guidance(guidance.clone());
            tracing::info!(
                step,
                text_provider_ms = generation.latency_ms,
                hints = guidance.hints.len(),
                artifacts = guidance.artifacts.len(),
                guidance_revision = state.guidance_revision,
                "task guidance created"
            );
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
        AgentAction::Wait { duration } => {
            tokio::time::sleep(Duration::from_millis(duration.milliseconds())).await;
            Ok((
                ActionStatus::Executed,
                InputResult::NotApplicable,
                VerificationResult::NotApplicable,
                None,
            ))
        }
        AgentAction::Commit {
            preview_artifact: _,
            operation,
        } => {
            let delegated = match operation {
                crate::action::CommitOperationResolved::VisualAction {
                    instruction_artifact,
                } => AgentAction::VisualAction {
                    instruction_artifact: instruction_artifact.clone(),
                },
                crate::action::CommitOperationResolved::PressKey { key } => {
                    AgentAction::PressKey { key: key.clone() }
                }
                crate::action::CommitOperationResolved::Hotkey { shortcut } => {
                    AgentAction::Hotkey {
                        shortcut: shortcut.clone(),
                    }
                }
            };
            Box::pin(execute_action(
                step,
                &delegated,
                state,
                services,
                portal,
                static_frame,
                debug,
                execute,
                settle_ms,
            ))
            .await
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

/// Verification is best-effort evidence. A missing PipeWire frame never
/// rewrites a successfully delivered portal input into an input failure.
async fn verify_input(
    portal: Option<&PortalSession>,
    static_frame: Option<&Frame>,
    before: Option<Frame>,
    settle_ms: u64,
) -> VerificationResult {
    tokio::time::sleep(Duration::from_millis(settle_ms)).await;
    match (before, current_frame(portal, static_frame).ok()) {
        (Some(before), Some(after)) => screen_delta(&before, &after),
        _ => VerificationResult::Unknown,
    }
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
fn record(
    state: &mut AgentState,
    step: usize,
    action: AgentAction,
    hint_id: Option<crate::action::GuidanceHintId>,
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
        hint_id,
        status,
        input_result: input,
        verification,
        started_at_ms: now_ms(),
        duration_ms: started.elapsed().as_secs_f64() * 1000.0,
        error,
        state_revision: state.state_revision,
        observation_revision: state.observation_revision,
        guidance_revision: state.guidance_revision,
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

async fn wait_for_cancel(control: &RunControl) {
    while !control.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(50)).await;
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
}

use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::{
    action::{ActionRecord, ActionStatus, AgentAction},
    config::Config,
    debug::DebugWriter,
    desktop::{Frame, PortalSession},
    jev::JevClient,
    skills::Skill,
    vision::VocaelaClient,
};

#[derive(Clone, Debug)]
pub enum AgentEvent {
    Working { step: usize, message: String },
    Error(String),
    Finished,
}

pub type EventSink = Arc<dyn Fn(AgentEvent) + Send + Sync>;

pub async fn run_goal(config: Config, goal: String, sink: EventSink) -> Result<()> {
    let skill = Skill::parse(&goal).map_err(|e| anyhow::anyhow!(e))?;
    let client = Client::builder().build()?;
    let jev = JevClient::new(
        client.clone(),
        config.typesafe_base_url.clone(),
        config.typesafe_model.clone(),
        config.typesafe_api_key.clone(),
        config.jev_min_confidence,
    );
    let vocaela = VocaelaClient::new(client, config.vocaela_url.clone());
    vocaela
        .health_check()
        .await
        .context("Vocaela health check failed")?;

    let debug = DebugWriter::new(&config.debug_dir).context("create debug run directory")?;
    let static_frame = match &config.image {
        Some(path) => Some(load_frame(path)?),
        None => None,
    };
    let portal = if static_frame.is_none() {
        Some(
            PortalSession::start()
                .await
                .context("start persistent Wayland portal session")?,
        )
    } else {
        None
    };
    let mut actions: Vec<AgentAction> = Vec::new();
    let mut history: Vec<Value> = Vec::new();
    let mut records: Vec<ActionRecord> = Vec::new();

    for step in 0..config.max_steps {
        let candidates = skill.candidates(&actions);
        if candidates.is_empty() {
            close_portal(&portal).await;
            anyhow::bail!("skill produced no candidate actions at step {step}");
        }
        let step_started = Instant::now();
        emit(
            &sink,
            AgentEvent::Working {
                step: step + 1,
                message: "Asking Jev for the next action".into(),
            },
        );
        let decision = match jev.next_action(&goal, &skill, &history, &candidates).await {
            Ok(decision) => decision,
            Err(error) => {
                emit(&sink, AgentEvent::Error(error.to_string()));
                close_portal(&portal).await;
                anyhow::bail!(error);
            }
        };
        tracing::info!(step = step + 1, action = %decision.choice, confidence = decision.confidence, probabilities = ?decision.probabilities, request_id = ?decision.request_id, jev_ms = decision.latency_ms, "Jev decision");
        let action = decision.action.clone();
        let action_started = now_string();
        if matches!(action, AgentAction::Done) {
            records.push(ActionRecord {
                step: step + 1,
                action: action.clone(),
                status: ActionStatus::Completed,
                started_at: action_started,
                duration_ms: step_started.elapsed().as_secs_f64() * 1000.0,
                error: None,
            });
            emit(&sink, AgentEvent::Finished);
            close_portal(&portal).await;
            tracing::info!(
                step = step + 1,
                total_step_ms = step_started.elapsed().as_secs_f64() * 1000.0,
                "goal completed"
            );
            return Ok(());
        }
        if let AgentAction::Abort { reason } = &action {
            let message = format!("Jev aborted: {reason}");
            records.push(ActionRecord {
                step: step + 1,
                action: action.clone(),
                status: ActionStatus::Failed,
                started_at: action_started,
                duration_ms: step_started.elapsed().as_secs_f64() * 1000.0,
                error: Some(message.clone()),
            });
            emit(&sink, AgentEvent::Error(message.clone()));
            close_portal(&portal).await;
            anyhow::bail!(message);
        }

        let outcome = execute_action(
            &action,
            &goal,
            step + 1,
            &vocaela,
            portal.as_ref(),
            static_frame.as_ref(),
            &debug,
            &records,
            config.execute,
        )
        .await;
        match outcome {
            Ok((status, input_ms)) => {
                let duration_ms = step_started.elapsed().as_secs_f64() * 1000.0;
                let history_status = serde_json::to_value(&status)
                    .unwrap_or_else(|_| Value::String("completed".into()));
                tracing::info!(
                    step = step + 1,
                    input_ms,
                    total_step_ms = duration_ms,
                    "action finished"
                );
                records.push(ActionRecord {
                    step: step + 1,
                    action: action.clone(),
                    status,
                    started_at: action_started,
                    duration_ms,
                    error: None,
                });
                actions.push(action.clone());
                history.push(serde_json::json!({
                    "step": step + 1,
                    "action": action,
                    "status": history_status,
                    "duration_ms": duration_ms,
                }));
                tokio::time::sleep(Duration::from_millis(config.action_settle_ms)).await;
            }
            Err(error) => {
                let message = error.to_string();
                let duration_ms = step_started.elapsed().as_secs_f64() * 1000.0;
                records.push(ActionRecord {
                    step: step + 1,
                    action: action.clone(),
                    status: ActionStatus::Failed,
                    started_at: action_started,
                    duration_ms,
                    error: Some(message.clone()),
                });
                emit(&sink, AgentEvent::Error(message.clone()));
                close_portal(&portal).await;
                anyhow::bail!(message);
            }
        }
    }
    close_portal(&portal).await;
    anyhow::bail!("maximum step count ({}) reached", config.max_steps)
}

async fn close_portal(portal: &Option<PortalSession>) {
    if let Some(portal) = portal {
        if let Err(error) = portal.close().await {
            tracing::warn!(%error, "failed to close portal session cleanly");
        }
    }
}

async fn execute_action(
    action: &AgentAction,
    _goal: &str,
    step: usize,
    vocaela: &VocaelaClient,
    portal: Option<&PortalSession>,
    static_frame: Option<&Frame>,
    debug: &DebugWriter,
    records: &[ActionRecord],
    execute: bool,
) -> Result<(ActionStatus, f64)> {
    match action {
        AgentAction::ClickTarget { target } => {
            let capture_started = Instant::now();
            let frame = if let Some(frame) = static_frame {
                frame.clone()
            } else {
                portal
                    .context("live capture is not initialized")?
                    .capture_latest()
                    .context("capture latest frame")?
            };
            let capture_ms = capture_started.elapsed().as_secs_f64() * 1000.0;
            let mut grounding = vocaela
                .ground(&frame, target)
                .await
                .context("grounding failed")?;
            grounding.frame_acquire_ms = capture_ms;
            let (pixel_x, pixel_y) = frame.normalized_to_pixel(grounding.bbox);
            tracing::info!(
                step,
                target,
                capture_ms,
                vocaela_ms = grounding.grounding_total_ms,
                png_encode_ms = grounding.png_encode_ms,
                base64_ms = grounding.base64_ms,
                vocaela_http_ms = grounding.vocaela_http_ms,
                vocaela_parse_ms = grounding.vocaela_parse_ms,
                grounding_total_ms = grounding.grounding_total_ms,
                normalized_x = grounding.bbox.center_x,
                normalized_y = grounding.bbox.center_y,
                pixel_x,
                pixel_y,
                "Vocaela grounding"
            );
            tracing::debug!(step, response = %grounding.raw_response, "Vocaela raw grounding response");
            debug
                .write_step(
                    step,
                    &frame,
                    Some(grounding.bbox),
                    target,
                    records,
                    Some(&grounding.png),
                )
                .context("write grounding debug artifacts")?;
            if execute {
                let input_started = Instant::now();
                portal
                    .context("--execute requires live portal capture")?
                    .click(&frame, grounding.bbox)
                    .await
                    .context("portal click failed")?;
                Ok((
                    ActionStatus::Completed,
                    input_started.elapsed().as_secs_f64() * 1000.0,
                ))
            } else {
                Ok((ActionStatus::Simulated, 0.0))
            }
        }
        AgentAction::TypeText { text } => {
            let started = Instant::now();
            if execute {
                portal
                    .context("--execute requires live portal capture")?
                    .type_text(text)
                    .await
                    .context("portal text input failed")?;
            }
            tracing::info!(
                step,
                input_ms = started.elapsed().as_secs_f64() * 1000.0,
                simulated = !execute,
                "text input"
            );
            Ok((
                if execute {
                    ActionStatus::Completed
                } else {
                    ActionStatus::Simulated
                },
                started.elapsed().as_secs_f64() * 1000.0,
            ))
        }
        AgentAction::PressEnter => {
            let started = Instant::now();
            if execute {
                portal
                    .context("--execute requires live portal capture")?
                    .press_enter()
                    .await
                    .context("portal Enter input failed")?;
            }
            tracing::info!(
                step,
                input_ms = started.elapsed().as_secs_f64() * 1000.0,
                simulated = !execute,
                "Enter input"
            );
            Ok((
                if execute {
                    ActionStatus::Completed
                } else {
                    ActionStatus::Simulated
                },
                started.elapsed().as_secs_f64() * 1000.0,
            ))
        }
        AgentAction::Done | AgentAction::Abort { .. } => unreachable!(),
    }
}

fn load_frame(path: &Path) -> Result<Frame> {
    Ok(Frame::from_image(image::open(path).with_context(|| {
        format!("read image {}", path.display())
    })?))
}

fn now_string() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    millis.to_string()
}

fn emit(sink: &EventSink, event: AgentEvent) {
    sink(event);
}

/// Used by the UI to ensure only one task is driving the portal at a time.
pub fn run_gate() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(1))
}

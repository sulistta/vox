mod action;
mod app_catalog;
mod config;
mod controller;
mod debug;
mod desktop;
mod jev;
mod policy;
mod secrets;
mod settings;
mod text_provider;
mod vision;

mod ui {
    slint::include_modules!();
}

use anyhow::Result;
use clap::Parser;
use config::{Cli, Config};
use controller::{
    AgentEvent, ConfirmationRequest, EventSink, RunControl, run_gate, run_goal_cancelable,
};
use jev::{DecisionProvider, JevClient, JevConfig};
use secrets::{KeyringSecretStore, SecretStore};
use settings::{SettingsStore, TextProviderSettings};
use slint::{ComponentHandle, SharedString};
use std::sync::{Arc, Mutex};
use text_provider::{OpenAiCompatibleTextProvider, TextProvider};

fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vocs=info".into()),
        )
        .init();
    let cli = Cli::parse();
    let config = Arc::new(Mutex::new(Config::from_cli(cli)?));
    let initial_goal = config.lock().unwrap().goal.clone();
    let runtime = tokio::runtime::Runtime::new()?;
    let handle = runtime.handle().clone();
    let dock = ui::AgentDock::new()?;
    dock.set_status_text("Ready".into());
    let gate = run_gate();
    let pending_confirmation: Arc<Mutex<Option<ConfirmationRequest>>> = Arc::new(Mutex::new(None));
    let active_run: Arc<Mutex<Option<RunControl>>> = Arc::new(Mutex::new(None));

    let start_handle = handle.clone();
    let start: Arc<dyn Fn(String) + Send + Sync> = {
        let dock_weak = dock.as_weak();
        let config = config.clone();
        let gate = gate.clone();
        let pending_confirmation = pending_confirmation.clone();
        let active_run = active_run.clone();
        Arc::new(move |goal: String| {
            let Ok(permit) = gate.clone().try_acquire_owned() else {
                if let Some(dock) = dock_weak.upgrade() {
                    dock.set_status_text("Working...".into());
                }
                return;
            };
            let sink = make_sink(dock_weak.clone(), pending_confirmation.clone());
            let run_config = config.lock().unwrap().clone();
            let control = RunControl::default();
            *active_run.lock().unwrap() = Some(control.clone());
            start_handle.spawn(async move {
                let result = run_goal_cancelable(run_config, goal, sink.clone(), control).await;
                drop(permit);
                if let Err(error) = result {
                    sink(AgentEvent::Error(error.to_string()));
                }
            });
        })
    };
    {
        let start = start.clone();
        let weak = dock.as_weak();
        dock.on_submit(move |text: SharedString| {
            let goal = text.trim().to_string();
            if !goal.is_empty() {
                if let Some(dock) = weak.upgrade() {
                    dock.set_composer_open(false);
                    dock.set_composer_text("".into());
                }
                start(goal);
            }
        });
    }
    {
        let weak = dock.as_weak();
        dock.on_chat(move || {
            if let Some(dock) = weak.upgrade() {
                dock.set_composer_open(true);
            }
        });
    }
    {
        let weak = dock.as_weak();
        dock.on_close_composer(move || {
            if let Some(dock) = weak.upgrade() {
                dock.set_composer_open(false);
            }
        });
    }
    dock.on_voice(|| tracing::info!("Voice is not implemented in the MVP"));
    {
        let active_run = active_run.clone();
        let pending = pending_confirmation.clone();
        dock.on_cancel_task(move || {
            if let Some(control) = active_run.lock().unwrap().as_ref() {
                control.cancel();
            }
            if let Some(request) = pending.lock().unwrap().take() {
                request.respond(false);
            }
        });
    }
    {
        let pending = pending_confirmation.clone();
        let weak = dock.as_weak();
        dock.on_allow_once(move || {
            if let Some(request) = pending.lock().unwrap().take() {
                request.respond(true);
            }
            if let Some(dock) = weak.upgrade() {
                dock.set_confirmation_open(false);
            }
        });
    }
    {
        let pending = pending_confirmation.clone();
        let weak = dock.as_weak();
        dock.on_cancel_confirmation(move || {
            if let Some(request) = pending.lock().unwrap().take() {
                request.respond(false);
            }
            if let Some(dock) = weak.upgrade() {
                dock.set_confirmation_open(false);
            }
        });
    }

    {
        let config = config.clone();
        let handle = handle.clone();
        dock.on_settings(move || {
            let window = match ui::SettingsWindow::new() {
                Ok(window) => window,
                Err(error) => {
                    tracing::error!(%error, "could not create settings window");
                    return;
                }
            };
            if let Err(error) = configure_settings_window(&window, &config, &handle) {
                tracing::error!(%error, "could not configure settings window");
                return;
            }
            if let Err(error) = window.show() {
                tracing::error!(%error, "could not show settings window");
            }
            // `show()` owns the visible component. Do not retain this strong
            // handle after the callback: Winit must be able to destroy the
            // native Settings window when the user closes it.
        });
    }
    if let Some(goal) = initial_goal {
        start(goal);
    }
    dock.run()?;
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    Ok(())
}

fn configure_settings_window(
    window: &ui::SettingsWindow,
    config: &Arc<Mutex<Config>>,
    handle: &tokio::runtime::Handle,
) -> Result<()> {
    {
        let current = config.lock().unwrap();
        window.set_jev_base_url(current.settings.jev.base_url.clone().into());
        window.set_jev_model(current.settings.jev.model.clone().into());
        if let Some(text) = &current.settings.text_provider {
            window.set_text_base_url(text.base_url.clone().into());
            window.set_text_model(text.model.clone().into());
        }
        window.set_status_text(if current.typesafe_api_key.is_some() {
            "Jev configured".into()
        } else {
            "Decision engine not configured".into()
        });
    }
    let store = SettingsStore::xdg()?;
    {
        let weak = window.as_weak();
        let config = config.clone();
        window.on_save(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let mut current = config.lock().unwrap();
            current.settings.jev.base_url = window.get_jev_base_url().to_string();
            current.settings.jev.model = window.get_jev_model().to_string();
            let base_url = window.get_text_base_url().trim().to_string();
            let model = window.get_text_model().trim().to_string();
            current.settings.text_provider =
                (!base_url.is_empty() && !model.is_empty()).then_some(TextProviderSettings {
                    base_url,
                    model,
                    timeout_ms: 30_000,
                });
            let secrets = KeyringSecretStore;
            let jev_key = window.get_jev_api_key().trim().to_string();
            if !jev_key.is_empty() {
                if let Err(error) = secrets.set("typesafe_api_key", &jev_key) {
                    window.set_status_text(format!("Could not save Jev secret: {error}").into());
                    return;
                }
                current.typesafe_api_key = Some(jev_key);
            }
            let text_key = window.get_text_api_key().trim().to_string();
            if !text_key.is_empty() {
                if let Err(error) = secrets.set("text_provider_api_key", &text_key) {
                    window.set_status_text(format!("Could not save text secret: {error}").into());
                    return;
                }
                current.text_provider_api_key = Some(text_key);
            }
            match store.save(&current.settings) {
                Ok(()) => {
                    window.set_jev_api_key("".into());
                    window.set_text_api_key("".into());
                    window.set_status_text("Saved. API keys are in the system keyring.".into());
                }
                Err(error) => {
                    window.set_status_text(format!("Could not save settings: {error}").into())
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        let config = config.clone();
        let handle = handle.clone();
        window.on_test_jev(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            window.set_status_text("Testing Jev connection…".into());
            let weak = window.as_weak();
            let current = config.lock().unwrap().clone();
            handle.spawn(async move {
                let provider = JevClient::new(
                    reqwest::Client::new(),
                    JevConfig {
                        base_url: current.settings.jev.base_url,
                        model: current.settings.jev.model,
                        api_key: current.typesafe_api_key,
                        timeout: std::time::Duration::from_secs(30),
                    },
                );
                let result = provider.test_connection().await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(window) = weak.upgrade() {
                        window.set_status_text(match result {
                            Ok(()) => "Jev connected".into(),
                            Err(error) => format!("Jev error: {error}").into(),
                        });
                    }
                });
            });
        });
    }
    {
        let weak = window.as_weak();
        let config = config.clone();
        let handle = handle.clone();
        window.on_test_text(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            window.set_status_text("Testing text provider…".into());
            let weak = window.as_weak();
            let current = config.lock().unwrap().clone();
            handle.spawn(async move {
                let result = match current
                    .settings
                    .text_provider_config(current.text_provider_api_key)
                {
                    Some(config) => match OpenAiCompatibleTextProvider::new(config) {
                        Ok(provider) => provider.test_connection().await,
                        Err(error) => Err(error),
                    },
                    None => Err(text_provider::TextProviderError::NotConfigured),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(window) = weak.upgrade() {
                        window.set_status_text(match result {
                            Ok(()) => "Text provider connected".into(),
                            Err(error) => format!("Text provider error: {error}").into(),
                        });
                    }
                });
            });
        });
    }
    Ok(())
}

fn make_sink(
    weak: slint::Weak<ui::AgentDock>,
    pending_confirmation: Arc<Mutex<Option<ConfirmationRequest>>>,
) -> EventSink {
    Arc::new(move |event| {
        let weak = weak.clone();
        let pending_confirmation = pending_confirmation.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(dock) = weak.upgrade() {
                match event {
                    AgentEvent::Working { step, message } => {
                        dock.set_working(true);
                        dock.set_status_text(SharedString::from(format!(
                            "{message} (step {step})"
                        )));
                    }
                    AgentEvent::SetupRequired { message } => {
                        dock.set_working(false);
                        dock.set_status_text(message.into());
                    }
                    AgentEvent::AwaitingUser { question } => {
                        dock.set_working(false);
                        dock.set_status_text(format!("Awaiting user: {question}").into());
                    }
                    AgentEvent::AwaitingConfirmation(request) => {
                        dock.set_working(false);
                        let detail = request
                            .preview
                            .clone()
                            .unwrap_or_else(|| request.action.clone());
                        *pending_confirmation.lock().unwrap() = Some(request);
                        dock.set_confirmation_text(detail.into());
                        dock.set_confirmation_open(true);
                        dock.set_status_text("Confirmation needed".into());
                    }
                    AgentEvent::Reply(message) => {
                        dock.set_working(false);
                        dock.set_status_text(message.into());
                    }
                    AgentEvent::Error(message) => {
                        dock.set_working(false);
                        dock.set_status_text(format!("Error: {message}").into());
                    }
                    AgentEvent::Finished | AgentEvent::Cancelled => {
                        dock.set_working(false);
                        dock.set_status_text("Ready".into());
                    }
                }
            }
        });
    })
}

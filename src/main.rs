mod action;
mod config;
mod controller;
mod debug;
mod desktop;
mod jev;
mod skills;
mod vision;

mod ui {
    slint::include_modules!();
}

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use config::{Cli, Config};
use controller::{AgentEvent, EventSink, run_gate, run_goal};
use slint::{ComponentHandle, SharedString};

fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vocs=info".into()),
        )
        .init();
    let cli = Cli::parse();
    let config = Config::from_cli(cli)?;
    let initial_goal = config.goal.clone();
    let runtime = tokio::runtime::Runtime::new()?;
    let handle = runtime.handle().clone();
    let dock = ui::AgentDock::new()?;
    dock.set_status_text("Ready".into());
    let gate = run_gate();

    let start: Arc<dyn Fn(String) + Send + Sync> = {
        let dock_weak = dock.as_weak();
        let config = config.clone();
        let gate = gate.clone();
        Arc::new(move |goal: String| {
            let Ok(permit) = gate.clone().try_acquire_owned() else {
                if let Some(dock) = dock_weak.upgrade() {
                    dock.set_status_text("Working...".into());
                }
                return;
            };
            let sink = make_sink(dock_weak.clone());
            let run_config = config.clone();
            handle.spawn(async move {
                let result = run_goal(run_config, goal, sink.clone()).await;
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

    if let Some(goal) = initial_goal {
        start(goal);
    }
    dock.run()?;
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    Ok(())
}

fn make_sink(weak: slint::Weak<ui::AgentDock>) -> EventSink {
    Arc::new(move |event| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(dock) = weak.upgrade() {
                match event {
                    AgentEvent::Working { step, message } => {
                        dock.set_working(true);
                        dock.set_status_text(SharedString::from(format!(
                            "{message} (step {step})"
                        )));
                    }
                    AgentEvent::Error(message) => {
                        dock.set_working(false);
                        dock.set_status_text(SharedString::from(format!("Error: {message}")));
                    }
                    AgentEvent::Finished => {
                        dock.set_working(false);
                        dock.set_status_text("Ready".into());
                    }
                }
            }
        });
    })
}

use std::{env, path::PathBuf};

use anyhow::{Context, Result};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "vocs", about = "Native Wayland computer-use MVP")]
pub struct Cli {
    /// Start a task automatically after the UI is initialized.
    #[arg(long)]
    pub goal: Option<String>,
    /// Use one static image instead of the live portal capture.
    #[arg(long)]
    pub image: Option<PathBuf>,
    /// Allow real pointer and keyboard input. Without it, actions are simulated.
    #[arg(long)]
    pub execute: bool,
    #[arg(long, default_value = "runs")]
    pub debug_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub typesafe_base_url: String,
    pub typesafe_model: String,
    pub typesafe_api_key: String,
    pub vocaela_url: String,
    pub jev_min_confidence: f64,
    pub max_steps: usize,
    pub action_settle_ms: u64,
    pub execute: bool,
    pub goal: Option<String>,
    pub image: Option<PathBuf>,
    pub debug_dir: PathBuf,
}

impl Config {
    pub fn from_cli(cli: Cli) -> Result<Self> {
        if cli.image.is_some() && cli.execute {
            anyhow::bail!(
                "--image cannot be combined with --execute; static coordinates are dry-run only"
            );
        }
        let typesafe_api_key =
            env::var("TYPESAFE_API_KEY").context("TYPESAFE_API_KEY is required")?;
        let config = Self {
            typesafe_base_url: env::var("TYPESAFE_BASE_URL")
                .unwrap_or_else(|_| "https://api.typesafe.ai".into()),
            typesafe_model: env::var("TYPESAFE_MODEL").unwrap_or_else(|_| "jev-latest".into()),
            typesafe_api_key,
            vocaela_url: env::var("VOCAELA_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into()),
            jev_min_confidence: env::var("JEV_MIN_CONFIDENCE")
                .unwrap_or_else(|_| "0.55".into())
                .parse()
                .context("JEV_MIN_CONFIDENCE must be a number")?,
            max_steps: env::var("MAX_STEPS")
                .unwrap_or_else(|_| "10".into())
                .parse()
                .context("MAX_STEPS must be an integer")?,
            action_settle_ms: env::var("ACTION_SETTLE_MS")
                .unwrap_or_else(|_| "300".into())
                .parse()
                .context("ACTION_SETTLE_MS must be an integer")?,
            execute: cli.execute,
            goal: cli.goal,
            image: cli.image,
            debug_dir: cli.debug_dir,
        };
        if config.jev_min_confidence < 0.0 || config.jev_min_confidence > 1.0 {
            anyhow::bail!("JEV_MIN_CONFIDENCE must be between 0 and 1");
        }
        if config.max_steps == 0 {
            anyhow::bail!("MAX_STEPS must be greater than zero");
        }
        Ok(config)
    }
}

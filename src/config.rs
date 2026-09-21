use std::{env, path::PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use directories::ProjectDirs;

use crate::{
    secrets::{KeyringSecretStore, SecretStore},
    settings::{Settings, SettingsStore},
};

#[derive(Debug, Parser)]
#[command(name = "vocs", about = "Native Wayland computer-use MVP")]
pub struct Cli {
    #[arg(long)]
    pub goal: Option<String>,
    #[arg(long)]
    pub image: Option<PathBuf>,
    #[arg(long)]
    pub execute: bool,
    #[arg(long)]
    pub debug_dir: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub settings: Settings,
    pub typesafe_api_key: Option<String>,
    pub text_provider_api_key: Option<String>,
    pub vocaela_url: String,
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
        let store = SettingsStore::xdg().context("resolve XDG settings path")?;
        let mut settings = store.load().context("load settings")?;
        if !store.exists() {
            if let Ok(base_url) = env::var("TYPESAFE_BASE_URL") {
                settings.jev.base_url = base_url;
            }
            if let Ok(model) = env::var("TYPESAFE_MODEL") {
                settings.jev.model = model;
            }
        }
        let secret_store = KeyringSecretStore;
        let typesafe_api_key = secret_store
            .get("typesafe_api_key")
            .ok()
            .flatten()
            .or_else(|| env::var("TYPESAFE_API_KEY").ok());
        let text_provider_api_key = secret_store
            .get("text_provider_api_key")
            .ok()
            .flatten()
            .or_else(|| env::var("TEXT_PROVIDER_API_KEY").ok());
        let dirs = ProjectDirs::from("org", "vocs", "vocs").context("resolve XDG state path")?;
        let debug_dir = cli
            .debug_dir
            .unwrap_or_else(|| dirs.state_dir().unwrap_or(dirs.data_dir()).join("runs"));
        let max_steps = env::var("MAX_STEPS")
            .unwrap_or_else(|_| "10".into())
            .parse()
            .context("MAX_STEPS must be an integer")?;
        if max_steps == 0 {
            anyhow::bail!("MAX_STEPS must be greater than zero");
        }
        Ok(Self {
            settings,
            typesafe_api_key,
            text_provider_api_key,
            vocaela_url: env::var("VOCAELA_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into()),
            max_steps,
            action_settle_ms: env::var("ACTION_SETTLE_MS")
                .unwrap_or_else(|_| "300".into())
                .parse()
                .context("ACTION_SETTLE_MS must be an integer")?,
            execute: cli.execute,
            goal: cli.goal,
            image: cli.image,
            debug_dir,
        })
    }
}

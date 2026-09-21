use std::{fs, path::PathBuf, time::Duration};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{policy::PolicyConfig, text_provider::TextProviderConfig};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JevSettings {
    pub base_url: String,
    pub model: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextProviderSettings {
    pub base_url: String,
    pub model: String,
    pub timeout_ms: u64,
    #[serde(default)]
    pub max_completion_tokens: Option<u32>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DebugSettings {
    pub persist_screenshots: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub jev: JevSettings,
    pub text_provider: Option<TextProviderSettings>,
    pub policy: PolicyConfig,
    pub debug: DebugSettings,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            jev: JevSettings {
                base_url: "https://api.typesafe.ai".into(),
                model: "jev-latest".into(),
            },
            text_provider: None,
            policy: PolicyConfig::default(),
            debug: DebugSettings {
                persist_screenshots: true,
            },
        }
    }
}
impl Settings {
    pub fn text_provider_config(&self, api_key: Option<String>) -> Option<TextProviderConfig> {
        self.text_provider
            .as_ref()
            .map(|settings| TextProviderConfig {
                base_url: settings.base_url.clone(),
                model: settings.model.clone(),
                api_key,
                timeout: Duration::from_millis(settings.timeout_ms),
                max_completion_tokens: settings.max_completion_tokens,
                reasoning_effort: settings.reasoning_effort.clone(),
            })
    }
}
#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("could not resolve XDG directories")]
    Xdg,
    #[error("settings I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}
pub struct SettingsStore {
    path: PathBuf,
}
impl SettingsStore {
    pub fn xdg() -> Result<Self, SettingsError> {
        let dirs = ProjectDirs::from("org", "vocs", "vocs").ok_or(SettingsError::Xdg)?;
        Ok(Self {
            path: dirs.config_dir().join("settings.json"),
        })
    }
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
    pub fn load(&self) -> Result<Settings, SettingsError> {
        match fs::read(&self.path) {
            Ok(data) => Ok(serde_json::from_slice(&data)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(error.into()),
        }
    }
    pub fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_vec_pretty(settings)?)?;
        Ok(())
    }
    pub fn exists(&self) -> bool {
        self.path.exists()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_non_secret_settings() {
        let path =
            std::env::temp_dir().join(format!("vocs-settings-{}.json", uuid::Uuid::new_v4()));
        let store = SettingsStore::at(path.clone());
        let mut settings = Settings::default();
        settings.text_provider = Some(TextProviderSettings {
            base_url: "http://text".into(),
            model: "model".into(),
            timeout_ms: 1000,
            max_completion_tokens: None,
            reasoning_effort: None,
        });
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap().text_provider.unwrap().model, "model");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn legacy_policy_settings_are_accepted_without_affecting_policy() {
        let path =
            std::env::temp_dir().join(format!("vocs-settings-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(
            &path,
            r#"{"jev":{"base_url":"http://jev","model":"test"},"text_provider":null,"policy":{"low_confidence":0.6,"medium_confidence":0.8,"high_confidence":0.9},"debug":{"persist_screenshots":false}}"#,
        )
        .unwrap();
        let settings = SettingsStore::at(path.clone()).load().unwrap();
        assert_eq!(settings.policy, PolicyConfig::default());
        let _ = std::fs::remove_file(path);
    }
}

use std::collections::BTreeMap;

use gio::prelude::AppInfoExt;
use thiserror::Error;

use crate::action::{AppId, AppInfo};

#[derive(Debug, Error)]
pub enum AppCatalogError {
    #[error("application {0} is not available")]
    Missing(String),
    #[error("could not launch application: {0}")]
    Launch(String),
}

pub trait AppCatalog: Send + Sync {
    fn apps(&self) -> Vec<AppInfo>;
    fn launch(&self, id: &AppId) -> Result<(), AppCatalogError>;
}

#[derive(Default)]
pub struct GioAppCatalog {
    apps: BTreeMap<String, AppInfo>,
}

impl GioAppCatalog {
    pub fn discover() -> Self {
        let apps = gio::AppInfo::all()
            .into_iter()
            .filter(|app| app.should_show())
            .filter_map(|app| {
                app.id().map(|id| {
                    let id = id.to_string();
                    (
                        id.clone(),
                        AppInfo {
                            id: AppId(id),
                            display_name: app.name().to_string(),
                            description: app.description().map(|value| value.to_string()),
                        },
                    )
                })
            })
            .collect();
        Self { apps }
    }
}

impl AppCatalog for GioAppCatalog {
    fn apps(&self) -> Vec<AppInfo> {
        self.apps.values().cloned().collect()
    }
    fn launch(&self, id: &AppId) -> Result<(), AppCatalogError> {
        if !self.apps.contains_key(&id.0) {
            return Err(AppCatalogError::Missing(id.0.clone()));
        }
        gio::AppInfo::all()
            .into_iter()
            .find(|app| app.id().as_deref() == Some(id.0.as_str()))
            .ok_or_else(|| AppCatalogError::Missing(id.0.clone()))?
            .launch(&[], None::<&gio::AppLaunchContext>)
            .map_err(|error| AppCatalogError::Launch(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct MockCatalog {
        apps: Vec<AppInfo>,
    }
    impl AppCatalog for MockCatalog {
        fn apps(&self) -> Vec<AppInfo> {
            self.apps.clone()
        }
        fn launch(&self, id: &AppId) -> Result<(), AppCatalogError> {
            self.apps
                .iter()
                .any(|app| &app.id == id)
                .then_some(())
                .ok_or_else(|| AppCatalogError::Missing(id.0.clone()))
        }
    }
    #[test]
    fn mock_catalog_is_data_driven() {
        let catalog = MockCatalog {
            apps: vec![AppInfo {
                id: AppId("example.desktop".into()),
                display_name: "Example Chat".into(),
                description: None,
            }],
        };
        assert_eq!(catalog.apps()[0].display_name, "Example Chat");
        assert!(catalog.launch(&AppId("example.desktop".into())).is_ok());
    }
}

use serde::{Deserialize, Serialize};

use crate::action::{AgentAction, ArtifactId};

/// Kept in persisted settings for forward compatibility. The current action
/// vocabulary has no explicit commit operation, so policy deliberately does
/// not guess that typing or a key press is consequential.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyConfig {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    RequireConfirmation { preview_artifact: ArtifactId },
}

#[derive(Clone, Debug, Default)]
pub struct PolicyEngine;

impl PolicyEngine {
    pub fn new(_config: PolicyConfig) -> Self {
        Self
    }

    /// Policy never infers a commit from text, app names, or Enter. Only the
    /// explicit typed capability is consequential and therefore confirmable.
    pub fn evaluate(&self, action: &AgentAction) -> PolicyDecision {
        match action {
            AgentAction::Commit {
                preview_artifact, ..
            } => PolicyDecision::RequireConfirmation {
                preview_artifact: preview_artifact.clone(),
            },
            _ => PolicyDecision::Allow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::ArtifactId;

    #[test]
    fn typing_is_low_level_editing_not_an_implicit_commit() {
        assert_eq!(
            PolicyEngine::new(PolicyConfig::default()).evaluate(&AgentAction::TypeArtifact {
                artifact: ArtifactId("text_1".into()),
            }),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn explicit_commit_always_requires_confirmation() {
        let action = AgentAction::Commit {
            preview_artifact: ArtifactId("preview_1".into()),
            operation: crate::action::CommitOperationResolved::PressKey {
                key: crate::action::Key::Enter,
            },
        };
        assert_eq!(
            PolicyEngine::new(PolicyConfig::default()).evaluate(&action),
            PolicyDecision::RequireConfirmation {
                preview_artifact: ArtifactId("preview_1".into()),
            }
        );
    }
}

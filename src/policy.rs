use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::action::{AgentAction, RiskLevel};

fn default_low_probability() -> f64 {
    0.50
}
fn default_medium_probability() -> f64 {
    0.40
}
fn default_high_probability() -> f64 {
    0.60
}
fn default_action_margin() -> f64 {
    0.04
}

/// Calibration values for the selected Choice probability, not TypeSafe's
/// derived `confidence`. Confidence changes meaning as the number of choices
/// changes; selected probability and runner-up margin remain useful evidence
/// across dynamic closed sets.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyConfig {
    #[serde(default = "default_low_probability")]
    pub low_action_probability: f64,
    #[serde(default = "default_medium_probability")]
    pub medium_action_probability: f64,
    #[serde(default = "default_high_probability")]
    pub high_action_probability: f64,
    #[serde(default = "default_action_margin")]
    pub min_action_margin: f64,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            low_action_probability: default_low_probability(),
            medium_action_probability: default_medium_probability(),
            high_action_probability: default_high_probability(),
            min_action_margin: default_action_margin(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecisionEvidence {
    pub selected_probability: f64,
    pub runner_up_probability: f64,
    pub margin: f64,
    /// Preserved for diagnostics; never used as a threshold.
    pub confidence: f64,
}

impl DecisionEvidence {
    pub fn from_choice(
        selected: &str,
        probabilities: &BTreeMap<String, f64>,
        confidence: f64,
    ) -> Result<Self, String> {
        if !confidence.is_finite() {
            return Err("Jev confidence is not finite".into());
        }
        let selected_probability = probabilities
            .get(selected)
            .copied()
            .ok_or_else(|| format!("Jev probabilities omit selected action {selected}"))?;
        if !(0.0..=1.0).contains(&selected_probability) || !selected_probability.is_finite() {
            return Err("selected action probability is invalid".into());
        }
        let runner_up_probability = probabilities
            .iter()
            .filter(|(choice, _)| choice.as_str() != selected)
            .map(|(_, probability)| *probability)
            .filter(|probability| probability.is_finite() && (0.0..=1.0).contains(probability))
            .fold(0.0, f64::max);
        Ok(Self {
            selected_probability,
            runner_up_probability,
            margin: selected_probability - runner_up_probability,
            confidence,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    RequireConfirmation,
    Reject(String),
}

#[derive(Clone, Debug)]
pub struct PolicyEvaluation {
    pub effective_risk: RiskLevel,
    pub decision: PolicyDecision,
}

#[derive(Clone, Debug)]
pub struct PolicyEngine {
    config: PolicyConfig,
}

impl PolicyEngine {
    pub fn new(config: PolicyConfig) -> Self {
        Self { config }
    }

    pub fn evaluate(
        &self,
        action: &AgentAction,
        evidence: &DecisionEvidence,
        jev_risk: RiskLevel,
    ) -> PolicyEvaluation {
        let effective_risk = default_risk(action).max(jev_risk);
        let threshold = match effective_risk {
            RiskLevel::Low => self.config.low_action_probability,
            RiskLevel::Medium => self.config.medium_action_probability,
            RiskLevel::High => self.config.high_action_probability,
        };
        let decision = if evidence.selected_probability < threshold {
            PolicyDecision::Reject(format!(
                "selected action probability {:.2} is below the {effective_risk:?} policy threshold {threshold:.2}",
                evidence.selected_probability
            ))
        } else if evidence.margin < self.config.min_action_margin {
            PolicyDecision::Reject(format!(
                "selected action margin {:.2} is below the policy margin {:.2}",
                evidence.margin, self.config.min_action_margin
            ))
        } else if matches!(effective_risk, RiskLevel::Medium | RiskLevel::High) {
            PolicyDecision::RequireConfirmation
        } else {
            PolicyDecision::Allow
        };
        PolicyEvaluation {
            effective_risk,
            decision,
        }
    }
}

pub fn default_risk(action: &AgentAction) -> RiskLevel {
    match action {
        AgentAction::LaunchApp { .. }
        | AgentAction::Wait { .. }
        | AgentAction::Done
        | AgentAction::Abort
        | AgentAction::RequestText { .. }
        | AgentAction::AskUser { .. }
        | AgentAction::ReplyToUser { .. } => RiskLevel::Low,
        AgentAction::VisualAction { .. }
        | AgentAction::Scroll { .. }
        | AgentAction::PressKey { .. }
        | AgentAction::Hotkey { .. } => RiskLevel::Medium,
        AgentAction::TypeArtifact { .. } => RiskLevel::High,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{AppId, ArtifactId, Key};

    fn evidence(selected: f64, runner_up: f64) -> DecisionEvidence {
        DecisionEvidence {
            selected_probability: selected,
            runner_up_probability: runner_up,
            margin: selected - runner_up,
            confidence: 0.1,
        }
    }

    #[test]
    fn high_risk_needs_confirmation() {
        let result = PolicyEngine::new(PolicyConfig::default()).evaluate(
            &AgentAction::TypeArtifact {
                artifact: ArtifactId("text_1".into()),
            },
            &evidence(0.95, 0.02),
            RiskLevel::Low,
        );
        assert_eq!(result.decision, PolicyDecision::RequireConfirmation);
    }
    #[test]
    fn medium_risk_reaches_confirmation_with_moderate_probability() {
        let result = PolicyEngine::new(PolicyConfig::default()).evaluate(
            &AgentAction::PressKey { key: Key::Enter },
            &evidence(0.55, 0.20),
            RiskLevel::Low,
        );
        assert_eq!(result.decision, PolicyDecision::RequireConfirmation);
    }
    #[test]
    fn code_baseline_cannot_be_lowered() {
        let result = PolicyEngine::new(PolicyConfig::default()).evaluate(
            &AgentAction::LaunchApp {
                app_id: AppId("x".into()),
            },
            &evidence(0.95, 0.02),
            RiskLevel::High,
        );
        assert_eq!(result.effective_risk, RiskLevel::High);
        assert_eq!(result.decision, PolicyDecision::RequireConfirmation);
    }
    #[test]
    fn ambiguous_action_is_rejected() {
        let result = PolicyEngine::new(PolicyConfig::default()).evaluate(
            &AgentAction::Done,
            &evidence(0.51, 0.49),
            RiskLevel::Low,
        );
        assert!(matches!(result.decision, PolicyDecision::Reject(_)));
    }
    #[test]
    fn selected_probability_and_margin_are_extracted() {
        let probabilities =
            BTreeMap::from([("visual_action".into(), 0.81), ("scroll".into(), 0.12)]);
        let result = DecisionEvidence::from_choice("visual_action", &probabilities, 0.79).unwrap();
        assert_eq!(result.selected_probability, 0.81);
        assert!((result.margin - 0.69).abs() < f64::EPSILON);
    }
}

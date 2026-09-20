use std::collections::BTreeMap;
use std::time::Instant;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{action::AgentAction, skills::Skill};

#[derive(Debug, Error)]
pub enum JevError {
    #[error("TypeSafe request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid TypeSafe response: {0}")]
    InvalidResponse(String),
    #[error("Jev confidence {confidence:.3} is below configured minimum {minimum:.3}")]
    LowConfidence { confidence: f64, minimum: f64 },
}

#[derive(Clone)]
pub struct JevClient {
    client: Client,
    base_url: String,
    model: String,
    api_key: String,
    min_confidence: f64,
}

#[derive(Debug, Serialize)]
struct JevRequest {
    model: String,
    state: Value,
    questions: BTreeMap<String, ChoiceQuestion>,
}

#[derive(Debug, Serialize)]
struct ChoiceQuestion {
    #[serde(rename = "type")]
    kind: &'static str,
    instructions: &'static str,
    criteria: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct JevResponse {
    #[serde(alias = "choices")]
    answers: BTreeMap<String, ChoiceAnswer>,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChoiceAnswer {
    choice: String,
    #[serde(default)]
    probabilities: BTreeMap<String, f64>,
    confidence: f64,
}

#[derive(Debug)]
pub struct Decision {
    pub action: AgentAction,
    pub choice: String,
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: f64,
    pub request_id: Option<String>,
    pub latency_ms: f64,
}

impl JevClient {
    pub fn new(
        client: Client,
        base_url: String,
        model: String,
        api_key: String,
        min_confidence: f64,
    ) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').into(),
            model,
            api_key,
            min_confidence,
        }
    }

    pub async fn next_action(
        &self,
        goal: &str,
        skill: &Skill,
        history: &[Value],
        candidates: &[AgentAction],
    ) -> Result<Decision, JevError> {
        let mut criteria = BTreeMap::new();
        let mut candidate_names = Vec::new();
        for candidate in candidates {
            let (name, description) = candidate_criterion(skill, candidate);
            candidate_names.push((name.to_string(), candidate.clone()));
            criteria.insert(name.to_string(), description.to_string());
        }
        let state = serde_json::json!({ "goal": goal, "skill": skill.name(), "history": history });
        let request = JevRequest {
            model: self.model.clone(),
            state,
            questions: BTreeMap::from([(
                "next_action".into(),
                ChoiceQuestion {
                    kind: "choice",
                    instructions: "Choose exactly one next desktop action that best advances the current goal. Respect the action history and choose only a listed action.",
                    criteria,
                },
            )]),
        };
        let started = Instant::now();
        let response = self
            .client
            .post(format!("{}/v1/systemone", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await?
            .error_for_status()?;
        let parsed: JevResponse = response
            .json()
            .await
            .map_err(|e| JevError::InvalidResponse(e.to_string()))?;
        let answer = parsed
            .answers
            .get("next_action")
            .ok_or_else(|| JevError::InvalidResponse("missing answers.next_action".into()))?;
        if !(0.0..=1.0).contains(&answer.confidence) || !answer.confidence.is_finite() {
            return Err(JevError::InvalidResponse(
                "confidence is not finite and normalized".into(),
            ));
        }
        if answer.confidence < self.min_confidence {
            return Err(JevError::LowConfidence {
                confidence: answer.confidence,
                minimum: self.min_confidence,
            });
        }
        let action = candidate_names
            .into_iter()
            .find(|(name, _)| name == &answer.choice)
            .map(|(_, action)| action)
            .ok_or_else(|| {
                JevError::InvalidResponse(format!("choice {:?} was not a candidate", answer.choice))
            })?;
        Ok(Decision {
            action,
            choice: answer.choice.clone(),
            probabilities: answer.probabilities.clone(),
            confidence: answer.confidence,
            request_id: parsed.request_id,
            latency_ms: started.elapsed().as_secs_f64() * 1000.0,
        })
    }
}

fn candidate_criterion(skill: &Skill, action: &AgentAction) -> (&'static str, &'static str) {
    match action {
        AgentAction::ClickTarget { .. } => match skill {
            Skill::YoutubeSearch { .. } => ("click_search", "Focus the YouTube search field."),
            Skill::GenericClick { .. } => ("click_target", "Focus the requested visual target."),
        },
        AgentAction::TypeText { .. } => match skill {
            Skill::YoutubeSearch { .. } => (
                "type_query",
                "Type the requested query into the focused field.",
            ),
            Skill::GenericClick { .. } => (
                "type_text",
                "Type the requested text into the already-focused control.",
            ),
        },
        AgentAction::PressEnter => ("press_enter", "Submit the current input."),
        AgentAction::Done => ("done", "The task is complete."),
        AgentAction::Abort { .. } => ("abort", "The task cannot safely continue."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_fixture_deserializes() {
        let response: JevResponse = serde_json::from_str(r#"{
            "answers": {"next_action": {"choice":"done", "probabilities":{"done":0.9}, "confidence":0.9}},
            "request_id":"req-1"
        }"#).unwrap();
        assert_eq!(response.answers["next_action"].choice, "done");
        assert_eq!(response.request_id.as_deref(), Some("req-1"));
    }
}

use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::action::TextRequestKind;

#[derive(Clone, Debug)]
pub struct TextProviderConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
    pub max_completion_tokens: Option<u32>,
    pub reasoning_effort: Option<String>,
}
#[derive(Clone, Debug)]
pub struct TextRequest {
    pub kind: TextRequestKind,
    pub goal: String,
    pub context: Vec<String>,
    pub requirements: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct TextGeneration {
    pub text: String,
    pub latency_ms: f64,
}
#[derive(Debug, Error)]
pub enum TextProviderError {
    #[error("text provider is not configured")]
    NotConfigured,
    #[error("text provider request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("text provider returned an invalid response: {0}")]
    InvalidResponse(String),
    #[error(
        "text provider output was truncated (model={model}, completion_tokens={completion_tokens:?}, reasoning_present={reasoning_present})"
    )]
    OutputTruncated {
        model: String,
        completion_tokens: Option<u64>,
        reasoning_present: bool,
    },
}

#[async_trait]
pub trait TextProvider: Send + Sync {
    async fn generate(&self, request: TextRequest) -> Result<TextGeneration, TextProviderError>;
    async fn test_connection(&self) -> Result<(), TextProviderError>;
}

#[derive(Clone)]
pub struct OpenAiCompatibleTextProvider {
    client: Client,
    config: TextProviderConfig,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}
#[derive(Serialize)]
struct Message {
    role: &'static str,
    content: String,
}
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}
#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    completion_tokens: Option<u64>,
}
#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}
#[derive(Deserialize)]
struct ResponseMessage {
    content: Option<String>,
    #[serde(default)]
    refusal: Option<serde_json::Value>,
    #[serde(default)]
    reasoning: Option<serde_json::Value>,
    #[serde(default)]
    reasoning_content: Option<serde_json::Value>,
}

impl OpenAiCompatibleTextProvider {
    pub fn new(config: TextProviderConfig) -> Result<Self, TextProviderError> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(TextProviderError::Http)?;
        Ok(Self { client, config })
    }
    fn request_body(&self, request: &TextRequest) -> ChatRequest {
        ChatRequest {
            model: self.config.model.clone(),
            messages: vec![
                Message {
                    role: "system",
                    content: prompt_for(request.kind.clone()),
                },
                Message {
                    role: "user",
                    content: format!(
                        "Goal:\n{}\n\nCurrent relevant context:\n{}\n\nRequirements:\n{}",
                        request.goal,
                        request.context.join("\n"),
                        request.requirements.join("\n")
                    ),
                },
            ],
            temperature: 0.2,
            max_tokens: if self.config.max_completion_tokens.is_none() {
                Some(if matches!(request.kind, TextRequestKind::TaskGuidance) {
                    // Some OpenAI-compatible reasoning models account hidden
                    // reasoning against this budget. Guidance is bounded and
                    // normally short, but 1024 can end before its JSON begins.
                    2048
                } else {
                    256
                })
            } else {
                None
            },
            max_completion_tokens: self.config.max_completion_tokens,
            reasoning_effort: self.config.reasoning_effort.clone(),
        }
    }
    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }
}

fn response_text(model: &str, response: &ChatResponse) -> Result<String, TextProviderError> {
    let choice = response
        .choices
        .first()
        .ok_or_else(|| TextProviderError::InvalidResponse("choices is empty".into()))?;
    let content_present = choice
        .message
        .content
        .as_ref()
        .is_some_and(|content| !content.trim().is_empty());
    let reasoning_present =
        choice.message.reasoning.is_some() || choice.message.reasoning_content.is_some();
    let completion_tokens = response
        .usage
        .as_ref()
        .and_then(|usage| usage.completion_tokens);
    tracing::debug!(model, finish_reason = ?choice.finish_reason, content_present, reasoning_present, completion_tokens, "text provider response metadata");
    if choice.finish_reason.as_deref() == Some("length") {
        return Err(TextProviderError::OutputTruncated {
            model: model.into(),
            completion_tokens,
            reasoning_present,
        });
    }
    choice
        .message
        .content
        .as_ref()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .ok_or_else(|| {
            TextProviderError::InvalidResponse(format!(
                "choices[0].message.content is empty (finish_reason={:?}, refusal_present={}, reasoning_present={}, reasoning_content_present={})",
                choice.finish_reason,
                choice.message.refusal.is_some(),
                choice.message.reasoning.is_some(),
                choice.message.reasoning_content.is_some(),
            ))
        })
}

#[async_trait]
impl TextProvider for OpenAiCompatibleTextProvider {
    async fn generate(&self, request: TextRequest) -> Result<TextGeneration, TextProviderError> {
        let started = Instant::now();
        let body = self.request_body(&request);
        let mut builder = self.client.post(self.endpoint()).json(&body);
        if let Some(key) = &self.config.api_key {
            builder = builder.bearer_auth(key);
        }
        let response: ChatResponse = builder.send().await?.error_for_status()?.json().await?;
        let text = response_text(&self.config.model, &response)?;
        Ok(TextGeneration {
            text,
            latency_ms: started.elapsed().as_secs_f64() * 1000.0,
        })
    }
    async fn test_connection(&self) -> Result<(), TextProviderError> {
        self.generate(TextRequest {
            kind: TextRequestKind::TransformText,
            goal: "Return the word ready".into(),
            context: vec![],
            requirements: vec!["Return only text.".into()],
        })
        .await
        .map(|_| ())
    }
}

fn prompt_for(kind: TextRequestKind) -> String {
    let purpose = match kind {
        TextRequestKind::ComposeText => "Write the requested text for input.",
        TextRequestKind::ExtractTextToType => {
            "Extract only the text that should be entered into a focused control."
        }
        TextRequestKind::VisualInstruction => {
            "Convert the requested current subgoal into exactly one concise low-level visual GUI instruction. Name one visible target for one pointer interaction only; do not include typing, key presses, waits, or a second action."
        }
        TextRequestKind::ReplyToUser => "Write the reply that should be shown to the user.",
        TextRequestKind::AskClarification => {
            "Write one concise question to obtain missing information."
        }
        TextRequestKind::TransformText => "Transform the supplied text as requested.",
        TextRequestKind::TaskGuidance => {
            r#"Return only one valid JSON TaskGuidance object. No Markdown.

Top level:
{"objective":"...","constraints":["..."],"artifacts":[...],"hints":[...]}

Every hint MUST have exactly this nesting: {"id":"hint_id","action":ACTION,"terminal":false}.
Valid ACTION forms are:
{"type":"launch_app"}
{"type":"visual_action","artifact":"visual_artifact_id"}
{"type":"type_artifact","artifact":"text_artifact_id"}
{"type":"press_key","key":"enter"}
{"type":"hotkey","shortcut":"paste"}
{"type":"scroll","direction":"down"}
{"type":"ask_user","artifact":"question_artifact_id"}
{"type":"reply_to_user","artifact":"reply_artifact_id"}
{"type":"wait","duration":"short"}
{"type":"commit","preview":"preview_artifact_id","operation":{"type":"press_key","key":"enter"}}

Do not omit required fields: press_key always has key; hotkey always has shortcut; scroll always has direction; wait always has duration; commit always has preview and operation.
Allowed key values: enter, escape, tab, backspace, up, down, left, right.
Allowed shortcuts: select_all, copy, cut, paste. Allowed waits: short, medium, long.

Each artifact is {"id":"...","kind":"visual_instruction|text_to_type|assistant_reply|clarification_question|action_preview","content":CONTENT}.
CONTENT is either {"type":"literal","value":"..."} or {"type":"generate","kind":"compose_text|extract_text_to_type|visual_instruction|reply_to_user|ask_clarification|transform_text","purpose":"...","requirements":["..."]}.
Hints are unordered candidates, never a plan, cursor, dependency, or next step. Do not emit coordinates, app names, request_plan, request_replan, done, abort, next, or explanations. Prefer a current-screen visual action; use launch_app only for an explicit user launch request or GroundingNotFound evidence."#.into()
        }
    };
    format!(
        "You generate only text requested by the desktop agent.\n{purpose}\nReturn only the requested content. Do not explain or output coordinates."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_is_minimal_and_api_key_is_optional() {
        let provider = OpenAiCompatibleTextProvider::new(TextProviderConfig {
            base_url: "http://provider".into(),
            model: "text-model".into(),
            api_key: None,
            timeout: Duration::from_secs(1),
            max_completion_tokens: None,
            reasoning_effort: None,
        })
        .unwrap();
        let body = provider.request_body(&TextRequest {
            kind: TextRequestKind::VisualInstruction,
            goal: "open a conversation".into(),
            context: vec![],
            requirements: vec![],
        });
        let json = serde_json::to_value(body).unwrap();
        assert_eq!(json["model"], "text-model");
        assert!(json.get("tools").is_none());
        assert_eq!(json["max_tokens"], 256);
        assert!(
            json["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("coordinates")
        );
    }

    #[test]
    fn task_guidance_prompt_spells_out_required_action_operands() {
        let prompt = prompt_for(TextRequestKind::TaskGuidance);
        assert!(prompt.contains("press_key always has key"));
        assert!(prompt.contains("commit always has preview and operation"));
        assert!(prompt.contains("\"type\":\"press_key\",\"key\":\"enter\""));
    }

    #[test]
    fn task_guidance_reserves_budget_for_reasoning_and_json() {
        let provider = OpenAiCompatibleTextProvider::new(TextProviderConfig {
            base_url: "http://provider".into(),
            model: "text-model".into(),
            api_key: None,
            timeout: Duration::from_secs(1),
            max_completion_tokens: None,
            reasoning_effort: None,
        })
        .unwrap();
        let body = provider.request_body(&TextRequest {
            kind: TextRequestKind::TaskGuidance,
            goal: "goal".into(),
            context: vec![],
            requirements: vec![],
        });
        assert_eq!(body.max_tokens, Some(2048));
    }

    #[test]
    fn length_finish_reason_is_a_typed_truncation_error() {
        let response: ChatResponse = serde_json::from_value(serde_json::json!({
            "choices": [{"finish_reason": "length", "message": {"content": "", "reasoning": "hidden"}}],
            "usage": {"completion_tokens": 1024}
        }))
        .unwrap();
        assert!(matches!(
            response_text("example", &response),
            Err(TextProviderError::OutputTruncated {
                completion_tokens: Some(1024),
                reasoning_present: true,
                ..
            })
        ));
    }
}

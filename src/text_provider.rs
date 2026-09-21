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
    max_tokens: u32,
}
#[derive(Serialize)]
struct Message {
    role: &'static str,
    content: String,
}
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}
#[derive(Deserialize)]
struct ResponseMessage {
    content: Option<String>,
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
            max_tokens: 256,
        }
    }
    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }
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
        let text = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_ref())
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .ok_or_else(|| {
                TextProviderError::InvalidResponse("choices[0].message.content is empty".into())
            })?;
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
            "Convert the requested current subgoal into one concise low-level visual GUI instruction."
        }
        TextRequestKind::ReplyToUser => "Write the reply that should be shown to the user.",
        TextRequestKind::AskClarification => {
            "Write one concise question to obtain missing information."
        }
        TextRequestKind::TransformText => "Transform the supplied text as requested.",
    };
    format!(
        "You generate only text requested by the desktop agent.\n{purpose}\nReturn only text content. Do not propose actions, explain, plan, output coordinates, or produce multiple semantic goals."
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
        assert!(
            json["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("coordinates")
        );
    }
}

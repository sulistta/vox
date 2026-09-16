use serde_json::Value;
use thiserror::Error;

pub const PROTOCOL_VERSION: u64 = 1;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("message exceeds {MAX_MESSAGE_BYTES} bytes")]
    MessageTooLarge,
    #[error("invalid JSON: {0}")]
    InvalidJson(String),
    #[error("message must be a JSON object")]
    NotObject,
    #[error("message type is missing or invalid")]
    InvalidType,
    #[error("unknown IPC message type: {0}")]
    UnknownType(String),
    #[error("required field is missing or invalid: {0}")]
    InvalidField(&'static str),
    #[error("protocol version mismatch: {0}")]
    VersionMismatch(u64),
}

pub fn validate_line(line: &str) -> Result<Value, ProtocolError> {
    if line.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::MessageTooLarge);
    }
    let value: Value = serde_json::from_str(line)
        .map_err(|error| ProtocolError::InvalidJson(error.to_string()))?;
    let object = value.as_object().ok_or(ProtocolError::NotObject)?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or(ProtocolError::InvalidType)?;
    const TYPES: &[&str] = &[
        "initialize",
        "initialized",
        "session.open",
        "session.opened",
        "turn.start",
        "turn.cancel",
        "tool.result",
        "tool.started",
        "tool.execute.requested",
        "tool.completed",
        "approval.required",
        "state.changed",
        "message.delta",
        "run.completed",
        "run.failed",
        "run.cancelled",
        "heartbeat",
        "shutdown",
        "error",
    ];
    if !TYPES.contains(&kind) {
        return Err(ProtocolError::UnknownType(kind.to_string()));
    }
    match kind {
        "turn.start" => {
            require_string(object, "request_id")?;
            require_string(object, "session_id")?;
            require_string(object, "run_id")?;
            require_string(object, "content")?;
            if let Some(source) = object.get("source") {
                if !matches!(source.as_str(), Some("text" | "voice")) {
                    return Err(ProtocolError::InvalidField("source"));
                }
            }
            if object.get("model_ref").is_some() {
                require_string(object, "model_ref")?;
            }
            if let Some(context) = object.get("context") {
                let messages = context
                    .as_array()
                    .ok_or(ProtocolError::InvalidField("context"))?;
                if messages.len() > 100 {
                    return Err(ProtocolError::InvalidField("context"));
                }
                for message in messages {
                    let message = message
                        .as_object()
                        .ok_or(ProtocolError::InvalidField("context"))?;
                    if !matches!(
                        message.get("role").and_then(Value::as_str),
                        Some("system" | "user" | "assistant" | "tool")
                    ) {
                        return Err(ProtocolError::InvalidField("context.role"));
                    }
                    if message
                        .get("content")
                        .and_then(Value::as_str)
                        .is_none_or(|content| content.len() > 64 * 1024)
                    {
                        return Err(ProtocolError::InvalidField("context.content"));
                    }
                    if let Some(effect) = message.get("effect") {
                        if !matches!(
                            effect.as_str(),
                            Some("none" | "pending" | "applied" | "unknown")
                        ) {
                            return Err(ProtocolError::InvalidField("context.effect"));
                        }
                    }
                }
            }
        }
        "turn.cancel" => {
            require_string(object, "request_id")?;
            require_string(object, "run_id")?;
        }
        "heartbeat" => {
            require_string(object, "request_id")?;
        }
        "tool.result" => {
            require_string(object, "run_id")?;
            require_string(object, "call_id")?;
            require_string(object, "tool")?;
            let status = object
                .get("status")
                .and_then(Value::as_str)
                .ok_or(ProtocolError::InvalidField("status"))?;
            if !["success", "error", "cancelled", "unknown"].contains(&status) {
                return Err(ProtocolError::InvalidField("status"));
            }
        }
        "initialized" => {
            let version = object
                .get("protocol")
                .and_then(Value::as_u64)
                .ok_or(ProtocolError::InvalidField("protocol"))?;
            if version != PROTOCOL_VERSION {
                return Err(ProtocolError::VersionMismatch(version));
            }
        }
        _ => {}
    }
    Ok(value)
}

fn require_string(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<(), ProtocolError> {
    match object.get(key).and_then(Value::as_str) {
        Some(value) if !value.is_empty() && value.len() <= 128 => Ok(()),
        _ => Err(ProtocolError::InvalidField(key)),
    }
}

pub fn encode(value: &Value) -> Result<String, ProtocolError> {
    let line = serde_json::to_string(value)
        .map_err(|error| ProtocolError::InvalidJson(error.to_string()))?;
    if line.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::MessageTooLarge);
    }
    Ok(format!("{line}\n"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Idle,
    Receiving,
    Transcribing,
    Thinking,
    Executing,
    WaitingUser,
    AwaitingApproval,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Receiving => "receiving",
            Self::Transcribing => "transcribing",
            Self::Thinking => "thinking",
            Self::Executing => "executing",
            Self::WaitingUser => "waiting_user",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_turn() {
        let value = serde_json::json!({
            "type": "turn.start",
            "request_id": "request",
            "session_id": "session",
            "run_id": "run",
            "content": "hello"
        });
        assert!(validate_line(&value.to_string()).is_ok());
    }

    #[test]
    fn accepts_bounded_context_and_rejects_invalid_effect() {
        let valid = serde_json::json!({
            "type": "turn.start",
            "request_id": "request",
            "session_id": "session",
            "run_id": "run",
            "content": "hello",
            "context": [{"role":"system","content":"constraint"},{"role":"tool","content":"uncertain","effect":"unknown"}]
        });
        assert!(validate_line(&valid.to_string()).is_ok());
        let invalid = serde_json::json!({
            "type": "turn.start",
            "request_id": "request",
            "session_id": "session",
            "run_id": "run",
            "content": "hello",
            "context": [{"role":"tool","content":"x","effect":"success"}]
        });
        assert_eq!(
            validate_line(&invalid.to_string()),
            Err(ProtocolError::InvalidField("context.effect"))
        );
    }

    #[test]
    fn rejects_unknown_and_oversized_messages() {
        assert!(matches!(
            validate_line(r#"{"type":"nope"}"#),
            Err(ProtocolError::UnknownType(_))
        ));
        let line = format!(
            r#"{{"type":"message.delta","delta":"{}"}}"#,
            "x".repeat(MAX_MESSAGE_BYTES)
        );
        assert_eq!(validate_line(&line), Err(ProtocolError::MessageTooLarge));
    }

    #[test]
    fn accepts_and_validates_heartbeat() {
        assert!(validate_line(r#"{"type":"heartbeat","request_id":"hb-1"}"#).is_ok());
        assert_eq!(
            validate_line(r#"{"type":"heartbeat","request_id":""}"#),
            Err(ProtocolError::InvalidField("request_id"))
        );
    }

    #[test]
    fn terminal_states_are_explicit() {
        assert!(RunState::Completed.is_terminal());
        assert!(!RunState::Executing.is_terminal());
    }

    #[test]
    fn accepts_shared_turn_fixture() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/contracts/valid-turn.json"
        ));
        let message = validate_line(fixture.trim()).expect("shared fixture must be valid");
        assert_eq!(message["type"], "turn.start");
    }

    #[test]
    fn rejects_shared_incompatible_version_fixture() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/contracts/invalid-version.json"
        ));
        assert_eq!(
            validate_line(fixture.trim()),
            Err(ProtocolError::VersionMismatch(99))
        );
    }
}

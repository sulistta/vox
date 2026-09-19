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
    let kind = object.get("type").and_then(Value::as_str).ok_or(ProtocolError::InvalidType)?;
    let schema = schema().get("$defs").and_then(|defs| defs.get(kind))
        .ok_or_else(|| ProtocolError::UnknownType(kind.to_owned()))?;
    if matches!(kind, "initialize" | "initialized") {
        if let Some(version) = object.get("protocol").and_then(Value::as_u64) {
            if version != PROTOCOL_VERSION { return Err(ProtocolError::VersionMismatch(version)); }
        }
    }
    validate_schema(&value, schema, "message")?;
    Ok(value)
}

fn schema() -> &'static Value {
    static SCHEMA: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    SCHEMA.get_or_init(|| serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"), "/../../schemas/ipc.schema.json"
    ))).expect("checked-in IPC schema"))
}

// Deliberately small interpreter for the keywords used by our checked-in schema.
// Contract tests reject unsupported keywords, so additions cannot silently weaken validation.
fn validate_schema(value: &Value, schema: &'static Value, field: &'static str) -> Result<(), ProtocolError> {
    let invalid = || ProtocolError::InvalidField(field);
    if let Some(expected) = schema.get("const") {
        let equal = value == expected || (value.is_number() && expected.is_number() && value.as_f64() == expected.as_f64());
        if !equal { return Err(invalid()); }
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        if !options.contains(value) { return Err(invalid()); }
    }
    let declared_type = schema.get("type");
    if value.is_null() && declared_type.and_then(Value::as_array).is_some_and(|types| types.iter().any(|kind| kind == "null")) {
        return Ok(());
    }
    let kind = declared_type.and_then(Value::as_str).or_else(|| {
        declared_type.and_then(Value::as_array).and_then(|types| types.iter().filter_map(Value::as_str).find(|kind| *kind != "null"))
    });
    match kind {
        Some("string") => {
            let text = value.as_str().ok_or_else(invalid)?;
            if text.chars().count() < schema.get("minLength").and_then(Value::as_u64).unwrap_or(0) as usize
                || text.len() > schema.get("x-maxUtf8Bytes").and_then(Value::as_u64).unwrap_or(u64::MAX) as usize {
                return Err(invalid());
            }
        }
        Some("object") => {
            let object = value.as_object().ok_or_else(invalid)?;
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for key in required { let key = key.as_str().unwrap();
                    if !object.contains_key(key) { return Err(ProtocolError::InvalidField(key)); }
                }
            }
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                for (key, rule) in properties {
                    if let Some(value) = object.get(key) {
                        let key = match (field, key.as_str()) {
                            ("context", "effect") => "context.effect",
                            ("context", "role") => "context.role",
                            ("context", "content") => "context.content",
                            ("messages", "role") => "messages.role",
                            ("messages", "content") => "messages.content",
                            _ => key.as_str(),
                        };
                        validate_schema(value, rule, key)?;
                    }
                }
            }
        }
        Some("array") => {
            let items = value.as_array().ok_or_else(invalid)?;
            if items.len() < schema.get("minItems").and_then(Value::as_u64).unwrap_or(0) as usize
                || items.len() > schema.get("maxItems").and_then(Value::as_u64).unwrap_or(u64::MAX) as usize {
                return Err(invalid());
            }
            if let Some(rule) = schema.get("items") { for item in items { validate_schema(item, rule, field)?; } }
        }
        Some("boolean") => { if !value.is_boolean() { return Err(invalid()); } }
        Some(kind @ ("number" | "integer")) => {
            let number = value.as_f64().ok_or_else(invalid)?;
            if !number.is_finite() || (kind == "integer" && number.fract() != 0.0)
                || number < schema.get("minimum").and_then(Value::as_f64).unwrap_or(f64::NEG_INFINITY)
                || number > schema.get("maximum").and_then(Value::as_f64).unwrap_or(f64::INFINITY) { return Err(invalid()); }
        }
        None => {}
        _ => panic!("unsupported IPC schema type"),
    }
    Ok(())
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

    fn expand_repeats(value: &mut Value) {
        if let Some(text) = value.get("$repeat").and_then(Value::as_str) {
            *value = Value::String(text.repeat(value["count"].as_u64().unwrap() as usize));
        } else if let Some(items) = value.as_array_mut() {
            for item in items { expand_repeats(item); }
        } else if let Some(object) = value.as_object_mut() {
            for item in object.values_mut() { expand_repeats(item); }
        }
    }

    #[test]
    fn shared_boundary_cases() {
        let cases: Value = serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/contracts/boundary-cases.json"))).unwrap();
        for case in cases.as_array().unwrap() {
            let mut message = case["message"].clone();
            expand_repeats(&mut message);
            let line = case.get("line").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| message.to_string());
            let result = validate_line(&line);
            assert_eq!(result.is_ok(), case["expect"] == "valid", "{}: {:?}", case["name"], result.err());
        }
    }

    #[test]
    fn schema_uses_only_supported_keywords() {
        fn check(rule: &Value) {
            for key in rule.as_object().unwrap().keys() {
                assert!(["type", "const", "enum", "minLength", "maxLength", "x-maxUtf8Bytes", "minimum", "maximum", "required", "properties", "items", "minItems", "maxItems", "additionalProperties"].contains(&key.as_str()), "unsupported keyword {key}");
            }
            if let Some(extra) = rule.get("additionalProperties") { assert_eq!(extra, true); }
            if let Some(properties) = rule.get("properties") { for child in properties.as_object().unwrap().values() { check(child); } }
            if let Some(items) = rule.get("items") { check(items); }
        }
        for rule in schema()["$defs"].as_object().unwrap().values() { check(rule); }
    }

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
    fn accepts_only_explicitly_redacted_model_activity() {
        let valid = serde_json::json!({
            "type": "model.requested",
            "session_id": "session",
            "run_id": "run",
            "round": 1,
            "provider": "openai-compatible",
            "redacted": true,
            "messages": [{"role":"user", "content":"token=[REDACTED]"}]
        });
        assert!(validate_line(&valid.to_string()).is_ok());
        let invalid = serde_json::json!({
            "type": "model.requested",
            "session_id": "session",
            "run_id": "run",
            "round": 0,
            "provider": "openai-compatible",
            "redacted": false,
            "messages": []
        });
        assert!(validate_line(&invalid.to_string()).is_err());
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

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use thiserror::Error;
use vox_desktop_access::{query_snapshot, NativeDesktop, SemanticQuery, SemanticSnapshot};
use vox_policy::{Decision, PolicyEngine, PolicyError};
use vox_tool_runtime::{RuntimeConfig, ToolError, ToolRuntime};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrokerCall {
    pub call_id: String,
    pub run_id: String,
    pub tool: String,
    pub arguments: Value,
    pub authorization_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrokerResult {
    pub status: String,
    pub data: Value,
    pub error_code: Option<String>,
    pub error: Option<String>,
    pub retryable: bool,
    pub side_effect: String,
    pub verification: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub approval_id: String,
    pub expires_at: u64,
    pub args_hash: String,
    pub scope: String,
}

#[derive(Debug, Error)]
pub enum BrokerError {
    #[error("policy rejected call: {0}")]
    Policy(#[from] PolicyError),
    #[error("tool failed: {0}")]
    Tool(#[from] ToolError),
    #[error("arguments for {0} must be an object")]
    ArgumentsNotObject(String),
    #[error("desktop accessibility action failed: {0}")]
    Desktop(String),
}

pub struct Broker {
    policy: PolicyEngine,
    runtime: ToolRuntime,
    desktop: NativeDesktop,
}

impl Default for Broker {
    fn default() -> Self {
        Self::new(RuntimeConfig::default())
    }
}

impl Broker {
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            policy: PolicyEngine::default(),
            runtime: ToolRuntime::new(config),
            desktop: NativeDesktop::default(),
        }
    }

    pub fn execute(&self, call: &BrokerCall) -> Result<BrokerResult, BrokerError> {
        self.execute_with_cancel(call, &AtomicBool::new(false))
    }

    pub fn execute_with_cancel(
        &self,
        call: &BrokerCall,
        cancel: &AtomicBool,
    ) -> Result<BrokerResult, BrokerError> {
        match self.policy.authorize_ref(
            &call.tool,
            &call.arguments,
            call.authorization_ref.as_deref(),
        ) {
            Ok(Decision::Allow) => {}
            Ok(Decision::RequireApproval) | Err(PolicyError::ApprovalRequired) => {
                return Ok(Self::approval_required())
            }
            Err(PolicyError::InvalidApproval) => return Ok(Self::invalid_approval()),
            Err(error) => return Err(BrokerError::Policy(error)),
        }
        let result = match call.tool.as_str() {
            "desktop.list_windows" => self.runtime.list_windows()?,
            "desktop.snapshot" => {
                let snapshots = self.desktop.snapshots().map_err(BrokerError::Desktop)?;
                vox_tool_runtime::ToolResult {
                    status: "success".into(),
                    data: serde_json::json!({"snapshots":snapshots}),
                    error_code: None,
                    retryable: false,
                    side_effect: "none".into(),
                    duration_ms: 0,
                    truncated: false,
                    verification: Some(serde_json::json!({"observed":true,"backend":"xa11y-rust"})),
                }
            }
            "desktop.query" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let snapshot: SemanticSnapshot = serde_json::from_value(
                    object
                        .get("snapshot")
                        .cloned()
                        .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?,
                )
                .map_err(|_| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let query: SemanticQuery = serde_json::from_value(
                    object
                        .get("query")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({})),
                )
                .map_err(|_| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let page = query_snapshot(&snapshot, &query);
                vox_tool_runtime::ToolResult {
                    status: "success".into(),
                    data: serde_json::to_value(page).unwrap_or_else(|_| serde_json::json!({})),
                    error_code: None,
                    retryable: false,
                    side_effect: "none".into(),
                    duration_ms: 0,
                    truncated: false,
                    verification: Some(serde_json::json!({"observed":true,"fresh":false})),
                }
            }
            "desktop.wait_for" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let query: SemanticQuery = serde_json::from_value(
                    object
                        .get("query")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({})),
                )
                .map_err(|_| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let timeout_ms = object
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(5_000)
                    .clamp(100, 30_000);
                let page = self
                    .desktop
                    .wait_for(&query, Duration::from_millis(timeout_ms))
                    .map_err(BrokerError::Desktop)?;
                let observed = !page.element_refs.is_empty();
                vox_tool_runtime::ToolResult {
                    status: if observed { "success" } else { "unknown" }.into(),
                    data: serde_json::to_value(page).unwrap_or_else(|_| serde_json::json!({})),
                    error_code: (!observed).then_some("POSTCONDITION_TIMEOUT".into()),
                    retryable: false,
                    side_effect: "none".into(),
                    duration_ms: timeout_ms,
                    truncated: false,
                    verification: Some(
                        serde_json::json!({"observed":observed,"requires_postcondition":!observed}),
                    ),
                }
            }
            "clipboard.read" => self.runtime.clipboard_read_with_cancel(cancel)?,
            "clipboard.write" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let text = object
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                self.runtime.clipboard_write_with_cancel(text, cancel)?
            }
            "desktop.act" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let snapshot: SemanticSnapshot = serde_json::from_value(
                    object
                        .get("snapshot")
                        .cloned()
                        .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?,
                )
                .map_err(|_| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let element_ref = object
                    .get("element_ref")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let action = object
                    .get("action")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let value = object.get("value").and_then(Value::as_str);
                let data = self
                    .desktop
                    .act(&snapshot, element_ref, action, value)
                    .map_err(BrokerError::Desktop)?;
                let verified = data
                    .get("verified")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                vox_tool_runtime::ToolResult {
                    status: if verified { "success" } else { "unknown" }.into(),
                    data,
                    error_code: None,
                    retryable: false,
                    side_effect: "unknown".into(),
                    duration_ms: 0,
                    truncated: false,
                    verification: Some(
                        serde_json::json!({"observed":verified,"requires_postcondition":!verified}),
                    ),
                }
            }
            "files.read" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                self.runtime.read_file(path)?
            }
            "files.search" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let root = object.get("root").and_then(Value::as_str).unwrap_or(".");
                let query = object
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let limit = object.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize;
                self.runtime.search_files(root, query, limit)?
            }
            "files.list" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let root = object.get("root").and_then(Value::as_str).unwrap_or(".");
                let limit = object.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize;
                self.runtime.list_dir(root, limit)?
            }
            "files.write" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let content = object
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let overwrite = object
                    .get("overwrite")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.runtime.write_file(path, content, overwrite)?
            }
            "files.move" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let source = object
                    .get("source")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let destination = object
                    .get("destination")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let overwrite = object
                    .get("overwrite")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.runtime.move_path(source, destination, overwrite)?
            }
            "process.list" => {
                let limit = call
                    .arguments
                    .as_object()
                    .and_then(|object| object.get("limit"))
                    .and_then(Value::as_u64)
                    .unwrap_or(200) as usize;
                self.runtime.list_processes(limit)?
            }
            "process.terminate" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let pid = object
                    .get("pid")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?
                    as u32;
                let expected = object.get("command").and_then(Value::as_str);
                let start_time = object.get("start_time").and_then(Value::as_u64);
                self.runtime
                    .terminate_process_with_identity(pid, expected, start_time)?
            }
            "apps.launch" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let program = object
                    .get("program")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let args = object
                    .get("argv")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                self.runtime.launch_app(program, &args)?
            }
            "apps.resolve" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                self.runtime.resolve_app(name)?
            }
            "paths.open" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                self.runtime.open_path(path)?
            }
            "shell.exec" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let program = object
                    .get("program")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let args = object
                    .get("argv")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let cwd = object.get("cwd").and_then(Value::as_str).map(Path::new);
                let timeout = object
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(30_000)
                    .clamp(100, 300_000);
                self.runtime.execute_argv(
                    program,
                    &args,
                    cwd,
                    Duration::from_millis(timeout),
                    cancel,
                )?
            }
            _ => {
                return Err(BrokerError::Policy(PolicyError::UnknownTool(
                    call.tool.clone(),
                )))
            }
        };
        Ok(BrokerResult {
            status: result.status,
            data: result.data,
            error_code: result.error_code,
            error: None,
            retryable: result.retryable,
            side_effect: result.side_effect,
            verification: result.verification,
        })
    }

    /// Create a short-lived approval for this exact call and execute it. The
    /// UI must call this only after the user has explicitly confirmed the
    /// displayed tool, target and consequence.
    pub fn execute_with_approval(&self, call: &BrokerCall) -> Result<BrokerResult, BrokerError> {
        let approval = self.request_approval(call)?;
        let mut authorized = call.clone();
        authorized.authorization_ref = Some(approval.approval_id);
        self.execute(&authorized)
    }

    pub fn request_approval(&self, call: &BrokerCall) -> Result<ApprovalRequest, BrokerError> {
        let token = self.policy.issue_approval(
            &call.tool,
            &call.arguments,
            format!("run:{}", call.run_id),
            Duration::from_secs(60),
        )?;
        Ok(ApprovalRequest {
            approval_id: token.approval_id.to_string(),
            expires_at: PolicyEngine::approval_expiry_epoch(&token),
            args_hash: token.args_hash,
            scope: token.scope,
        })
    }

    pub fn revoke_approval(&self, approval_id: &str) -> Result<(), BrokerError> {
        self.policy.revoke_approval(approval_id)?;
        Ok(())
    }

    fn approval_required() -> BrokerResult {
        BrokerResult {
            status: "error".into(),
            data: Value::Null,
            error_code: Some("APPROVAL_REQUIRED".into()),
            error: Some("a user approval scoped to this exact call is required".into()),
            retryable: false,
            side_effect: "none".into(),
            verification: None,
        }
    }

    fn invalid_approval() -> BrokerResult {
        BrokerResult {
            status: "error".into(),
            data: Value::Null,
            error_code: Some("INVALID_APPROVAL".into()),
            error: Some("approval is expired, mismatched, or already used".into()),
            retryable: false,
            side_effect: "none".into(),
            verification: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn read_tool_is_executed_only_by_the_broker() {
        let broker = Broker::default();
        let call = BrokerCall {
            call_id: "c".into(),
            run_id: "r".into(),
            tool: "shell.exec".into(),
            arguments: serde_json::json!({"program":"printf","argv":["ok"]}),
            authorization_ref: None,
        };
        let result = broker.execute(&call).unwrap();
        assert_eq!(result.error_code.as_deref(), Some("APPROVAL_REQUIRED"));
    }

    #[test]
    fn mismatched_approval_never_reaches_the_runtime() {
        let broker = Broker::default();
        let call = BrokerCall {
            call_id: "c".into(),
            run_id: "r".into(),
            tool: "shell.exec".into(),
            arguments: serde_json::json!({"program":"printf","argv":["ok"]}),
            authorization_ref: Some("not-a-token".into()),
        };
        let result = broker.execute(&call).unwrap();
        assert_eq!(result.error_code.as_deref(), Some("INVALID_APPROVAL"));
    }

    #[test]
    fn a05_existing_destination_never_overwrites_without_approval() {
        let root = std::env::temp_dir().join(format!(
            "vox-broker-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("approved.txt");
        let broker = Broker::new(RuntimeConfig {
            allowed_roots: vec![root.clone()],
            ..RuntimeConfig::default()
        });
        let call = BrokerCall {
            call_id: "call-write".into(),
            run_id: "run-write".into(),
            tool: "files.write".into(),
            arguments: serde_json::json!({
                "path": path,
                "content": "approved once",
                "overwrite": false
            }),
            authorization_ref: None,
        };

        let pending = broker.execute(&call).unwrap();
        assert_eq!(pending.error_code.as_deref(), Some("APPROVAL_REQUIRED"));
        assert!(!path.exists());

        let applied = broker.execute_with_approval(&call).unwrap();
        assert_eq!(applied.status, "success");
        assert_eq!(fs::read_to_string(&path).unwrap(), "approved once");

        let replay = broker.execute(&call).unwrap();
        assert_eq!(replay.error_code.as_deref(), Some("APPROVAL_REQUIRED"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "approved once");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a11_clipboard_write_requires_approval_and_uses_the_runtime_backend() {
        let root = std::env::temp_dir().join(format!(
            "vox-broker-clipboard-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let read_script = root.join("clipboard-read.sh");
        let write_script = root.join("clipboard-write.sh");
        let output_path = root.join("clipboard.txt");
        fs::write(&read_script, "#!/bin/sh\nprintf 'from-clipboard'\n").unwrap();
        fs::write(
            &write_script,
            format!("#!/bin/sh\ncat > '{}'\n", output_path.display()),
        )
        .unwrap();
        for script in [&read_script, &write_script] {
            let mut permissions = fs::metadata(script).unwrap().permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
            fs::set_permissions(script, permissions).unwrap();
        }
        let broker = Broker::new(RuntimeConfig {
            allowed_roots: vec![root.clone()],
            clipboard_read_command: Some(read_script),
            clipboard_write_command: Some(write_script),
            ..RuntimeConfig::default()
        });
        let read_call = BrokerCall {
            call_id: "clipboard-read".into(),
            run_id: "clipboard-run".into(),
            tool: "clipboard.read".into(),
            arguments: serde_json::json!({}),
            authorization_ref: None,
        };
        let read = broker.execute(&read_call).unwrap();
        assert_eq!(read.status, "success");
        assert_eq!(read.data["text"], "from-clipboard");

        let write_call = BrokerCall {
            call_id: "clipboard-write".into(),
            run_id: "clipboard-run".into(),
            tool: "clipboard.write".into(),
            arguments: serde_json::json!({"text":"approved clipboard"}),
            authorization_ref: None,
        };
        let pending = broker.execute(&write_call).unwrap();
        assert_eq!(pending.error_code.as_deref(), Some("APPROVAL_REQUIRED"));
        assert!(!output_path.exists());

        let applied = broker.execute_with_approval(&write_call).unwrap();
        assert_eq!(applied.status, "success");
        assert_eq!(
            fs::read_to_string(&output_path).unwrap(),
            "approved clipboard"
        );
        let _ = fs::remove_dir_all(root);
    }
}

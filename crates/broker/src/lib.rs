use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{atomic::AtomicBool, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;
use vox_desktop_access::{
    is_sensitive_semantic_node, query_snapshot, NativeDesktop, SemanticQuery, SemanticSnapshot,
};
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
    pub summary: Option<String>,
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
    #[error("desktop snapshot is missing, expired, already used, or belongs to another run")]
    SnapshotUnavailable,
}

const MAX_ISSUED_SNAPSHOTS: usize = 64;
const SNAPSHOT_TTL: Duration = Duration::from_secs(30);

fn optional_desktop_app_pid(
    arguments: &Map<String, Value>,
    tool: &str,
) -> Result<Option<u32>, BrokerError> {
    let Some(value) = arguments.get("app_pid") else {
        return Ok(None);
    };
    let pid = value
        .as_u64()
        .filter(|pid| *pid > 0)
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or_else(|| BrokerError::ArgumentsNotObject(tool.to_owned()))?;
    Ok(Some(pid))
}

#[derive(Debug, Clone)]
struct IssuedSnapshot {
    run_id: String,
    snapshot: SemanticSnapshot,
    issued_at: Instant,
}

/// Accessibility trees cross the model boundary as data, but the broker keeps
/// the authoritative copy. The model can therefore refer to a snapshot id
/// without being able to manufacture a tree, a target, or a previous run's
/// observation. An action consumes its snapshot and must obtain a new native
/// observation for any following effect.
#[derive(Default)]
struct SnapshotRegistry {
    snapshots: Mutex<BTreeMap<String, IssuedSnapshot>>,
}

impl SnapshotRegistry {
    fn replace_for_run(&self, run_id: &str, snapshots: impl IntoIterator<Item = SemanticSnapshot>) {
        let Ok(mut known) = self.snapshots.lock() else {
            return;
        };
        known.retain(|_, issued| {
            issued.run_id != run_id && issued.issued_at.elapsed() < SNAPSHOT_TTL
        });
        for snapshot in snapshots {
            known.insert(
                snapshot.snapshot_id.clone(),
                IssuedSnapshot {
                    run_id: run_id.to_owned(),
                    snapshot,
                    issued_at: Instant::now(),
                },
            );
        }
    }

    fn get(&self, run_id: &str, snapshot_id: &str) -> Result<SemanticSnapshot, BrokerError> {
        let mut known = self
            .snapshots
            .lock()
            .map_err(|_| BrokerError::SnapshotUnavailable)?;
        let Some(issued) = known.get(snapshot_id) else {
            return Err(BrokerError::SnapshotUnavailable);
        };
        if issued.issued_at.elapsed() >= SNAPSHOT_TTL {
            known.remove(snapshot_id);
            return Err(BrokerError::SnapshotUnavailable);
        }
        if issued.run_id != run_id {
            return Err(BrokerError::SnapshotUnavailable);
        }
        Ok(issued.snapshot.clone())
    }

    fn take_for_action(
        &self,
        run_id: &str,
        snapshot_id: &str,
    ) -> Result<SemanticSnapshot, BrokerError> {
        let mut known = self
            .snapshots
            .lock()
            .map_err(|_| BrokerError::SnapshotUnavailable)?;
        let Some(issued) = known.get(snapshot_id) else {
            return Err(BrokerError::SnapshotUnavailable);
        };
        if issued.issued_at.elapsed() >= SNAPSHOT_TTL {
            known.remove(snapshot_id);
            return Err(BrokerError::SnapshotUnavailable);
        }
        if issued.run_id != run_id {
            return Err(BrokerError::SnapshotUnavailable);
        }
        known
            .remove(snapshot_id)
            .map(|issued| issued.snapshot)
            .ok_or(BrokerError::SnapshotUnavailable)
    }
}

pub struct Broker {
    policy: PolicyEngine,
    runtime: ToolRuntime,
    desktop: NativeDesktop,
    snapshots: SnapshotRegistry,
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
            snapshots: SnapshotRegistry::default(),
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
            &format!("run:{}", call.run_id),
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
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let app_pid = optional_desktop_app_pid(object, &call.tool)?;
                let mut snapshots = self
                    .desktop
                    .snapshots_for_app_pid(app_pid)
                    .map_err(BrokerError::Desktop)?;
                let truncated = snapshots.len() > MAX_ISSUED_SNAPSHOTS;
                snapshots.truncate(MAX_ISSUED_SNAPSHOTS);
                self.snapshots
                    .replace_for_run(&call.run_id, snapshots.clone());
                vox_tool_runtime::ToolResult {
                    status: "success".into(),
                    data: serde_json::json!({"snapshots":snapshots,"app_pid":app_pid}),
                    error_code: None,
                    retryable: false,
                    side_effect: "none".into(),
                    duration_ms: 0,
                    truncated,
                    verification: Some(serde_json::json!({
                        "observed": true,
                        "backend": "xa11y-rust",
                        "app_pid": app_pid,
                        "snapshot_ttl_ms": SNAPSHOT_TTL.as_millis(),
                        "action_consumes_snapshot": true
                    })),
                }
            }
            "desktop.query" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let snapshot_id = object
                    .get("snapshot_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let snapshot = self.snapshots.get(&call.run_id, snapshot_id)?;
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
                let app_pid = optional_desktop_app_pid(object, &call.tool)?;
                let page = self
                    .desktop
                    .wait_for_with_cancel_for_app_pid(
                        &query,
                        Duration::from_millis(timeout_ms),
                        cancel,
                        app_pid,
                    )
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
                    verification: Some(serde_json::json!({
                        "observed":observed,
                        "requires_postcondition":!observed,
                        "app_pid":app_pid
                    })),
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
                let snapshot_id = object
                    .get("snapshot_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let element_ref = object
                    .get("element_ref")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let action = object
                    .get("action")
                    .and_then(Value::as_str)
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let value = object.get("value").and_then(Value::as_str);
                let snapshot = self.snapshots.take_for_action(&call.run_id, snapshot_id)?;
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
        let summary = self.approval_summary(call)?;
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
            summary,
        })
    }

    fn approval_summary(&self, call: &BrokerCall) -> Result<Option<String>, BrokerError> {
        let object = call
            .arguments
            .as_object()
            .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
        match call.tool.as_str() {
            "files.write" => {
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("arquivo");
                let verb = if object.get("overwrite").and_then(Value::as_bool) == Some(true) {
                    "sobrescrever"
                } else {
                    "criar ou escrever"
                };
                return Ok(Some(format!("{verb} {}", truncate_for_approval(path, 160))));
            }
            "files.move" => {
                let source = object
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or("origem");
                let destination = object
                    .get("destination")
                    .and_then(Value::as_str)
                    .unwrap_or("destino");
                return Ok(Some(format!(
                    "mover {} para {}",
                    truncate_for_approval(source, 96),
                    truncate_for_approval(destination, 96)
                )));
            }
            "clipboard.write" => return Ok(Some("alterar a área de transferência".into())),
            "process.terminate" => {
                let pid = object
                    .get("pid")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                return Ok(Some(format!("encerrar o processo {pid}")));
            }
            "apps.launch" => {
                let program = object
                    .get("program")
                    .and_then(Value::as_str)
                    .unwrap_or("aplicativo");
                return Ok(Some(format!(
                    "abrir {}",
                    truncate_for_approval(program, 160)
                )));
            }
            "paths.open" => {
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("caminho");
                return Ok(Some(format!("abrir {}", truncate_for_approval(path, 160))));
            }
            "shell.exec" => {
                let program = object
                    .get("program")
                    .and_then(Value::as_str)
                    .unwrap_or("programa");
                return Ok(Some(format!(
                    "executar {}",
                    truncate_for_approval(program, 160)
                )));
            }
            "desktop.act" => {}
            _ => return Ok(None),
        }
        let snapshot_id = object
            .get("snapshot_id")
            .and_then(Value::as_str)
            .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
        let element_ref = object
            .get("element_ref")
            .and_then(Value::as_str)
            .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
        let action = object
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
        let snapshot = self.snapshots.get(&call.run_id, snapshot_id)?;
        let node = snapshot
            .nodes
            .iter()
            .find(|node| node.element_ref == element_ref)
            .ok_or(BrokerError::SnapshotUnavailable)?;
        let sensitive = is_sensitive_semantic_node(node);
        if sensitive && matches!(action, "set_value" | "set-value") {
            return Err(BrokerError::Desktop(
                "editing a sensitive accessibility field is not supported".into(),
            ));
        }
        let name = if sensitive {
            "conteúdo sensível".into()
        } else if node.name.trim().is_empty() {
            node.role.clone()
        } else {
            truncate_for_approval(&node.name, 160)
        };
        Ok(Some(format!(
            "{} em {}",
            approval_action_label(action),
            name
        )))
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

fn truncate_for_approval(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let visible = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{visible}…")
    } else {
        visible
    }
}

fn approval_action_label(action: &str) -> &str {
    match action {
        "press" => "pressionar",
        "focus" => "focar",
        "toggle" => "alternar",
        "set_value" | "set-value" => "preencher",
        _ => action,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn issued_snapshot(snapshot_id: &str) -> SemanticSnapshot {
        SemanticSnapshot {
            snapshot_id: snapshot_id.into(),
            captured_at: "0".into(),
            generation: 1,
            app_id: "fixture-app:1".into(),
            window_id: "fixture-window".into(),
            focused_element: None,
            capabilities: vec!["query".into()],
            nodes: Vec::new(),
            truncated: false,
            continuation: None,
        }
    }

    #[test]
    fn desktop_app_pid_scope_requires_a_positive_u32() {
        let valid = serde_json::json!({"app_pid": 42});
        assert_eq!(
            optional_desktop_app_pid(valid.as_object().unwrap(), "desktop.snapshot").unwrap(),
            Some(42)
        );
        for invalid in [
            serde_json::json!({"app_pid": 0}),
            serde_json::json!({"app_pid": -1}),
            serde_json::json!({"app_pid": u64::from(u32::MAX) + 1}),
            serde_json::json!({"app_pid": "42"}),
        ] {
            assert!(matches!(
                optional_desktop_app_pid(invalid.as_object().unwrap(), "desktop.snapshot"),
                Err(BrokerError::ArgumentsNotObject(tool)) if tool == "desktop.snapshot"
            ));
        }
    }

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
    fn accessibility_snapshots_are_bound_to_a_run_and_consumed_by_actions() {
        let registry = SnapshotRegistry::default();
        registry.replace_for_run("run-a", [issued_snapshot("snapshot-a")]);

        assert_eq!(
            registry.get("run-a", "snapshot-a").unwrap().snapshot_id,
            "snapshot-a"
        );
        assert!(matches!(
            registry.get("run-b", "snapshot-a"),
            Err(BrokerError::SnapshotUnavailable)
        ));

        // A mismatched run cannot destroy the issuing run's observation.
        assert_eq!(
            registry
                .take_for_action("run-a", "snapshot-a")
                .unwrap()
                .snapshot_id,
            "snapshot-a"
        );
        assert!(matches!(
            registry.get("run-a", "snapshot-a"),
            Err(BrokerError::SnapshotUnavailable)
        ));
    }

    #[test]
    fn fabricated_snapshot_ids_are_rejected_before_desktop_access() {
        let broker = Broker::default();
        let call = BrokerCall {
            call_id: "query-fabricated".into(),
            run_id: "run-fabricated".into(),
            tool: "desktop.query".into(),
            arguments: serde_json::json!({
                "snapshot_id": "not-issued-by-broker",
                "query": {"role": "button"}
            }),
            authorization_ref: None,
        };
        assert!(matches!(
            broker.execute(&call),
            Err(BrokerError::SnapshotUnavailable)
        ));
    }

    #[test]
    fn desktop_approval_has_a_broker_derived_target_summary() {
        use vox_desktop_access::SemanticNode;

        let broker = Broker::default();
        let mut snapshot = issued_snapshot("snapshot-for-approval");
        snapshot.nodes.push(SemanticNode {
            element_ref: "button-save".into(),
            parent_ref: None,
            role: "button".into(),
            name: "Salvar alterações".into(),
            value: None,
            states: vec!["enabled".into()],
            actions: vec!["press".into()],
            bounds: None,
        });
        broker.snapshots.replace_for_run("run-approval", [snapshot]);
        let call = BrokerCall {
            call_id: "act-save".into(),
            run_id: "run-approval".into(),
            tool: "desktop.act".into(),
            arguments: serde_json::json!({
                "snapshot_id": "snapshot-for-approval",
                "element_ref": "button-save",
                "action": "press"
            }),
            authorization_ref: None,
        };

        let approval = broker.request_approval(&call).unwrap();
        assert_eq!(
            approval.summary.as_deref(),
            Some("pressionar em Salvar alterações")
        );
    }

    #[test]
    fn sensitive_accessibility_fields_cannot_be_filled_by_the_model() {
        use vox_desktop_access::SemanticNode;

        let broker = Broker::default();
        for (index, (name, states)) in [
            ("Password", vec!["password".into()]),
            ("Senha do provedor", vec!["enabled".into()]),
            ("Token de acesso", vec!["enabled".into()]),
        ]
        .into_iter()
        .enumerate()
        {
            let run_id = format!("run-sensitive-{index}");
            let snapshot_id = format!("snapshot-sensitive-{index}");
            let element_ref = format!("sensitive-field-{index}");
            let mut snapshot = issued_snapshot(&snapshot_id);
            snapshot.nodes.push(SemanticNode {
                element_ref: element_ref.clone(),
                parent_ref: None,
                role: "textfield".into(),
                name: name.into(),
                value: Some("[REDACTED]".into()),
                states,
                actions: vec!["set-value".into()],
                bounds: None,
            });
            broker.snapshots.replace_for_run(&run_id, [snapshot]);
            let call = BrokerCall {
                call_id: format!("set-sensitive-{index}"),
                run_id,
                tool: "desktop.act".into(),
                arguments: serde_json::json!({
                    "snapshot_id": snapshot_id,
                    "element_ref": element_ref,
                    "action": "set-value",
                    "value": "never-send-this"
                }),
                authorization_ref: None,
            };
            assert!(matches!(
                broker.request_approval(&call),
                Err(BrokerError::Desktop(message)) if message.contains("sensitive")
            ));
        }
    }

    #[test]
    fn file_approvals_describe_the_user_visible_target() {
        let broker = Broker::default();
        let call = BrokerCall {
            call_id: "write-readme".into(),
            run_id: "run-write".into(),
            tool: "files.write".into(),
            arguments: serde_json::json!({
                "path": "/tmp/vox/README.md",
                "content": "novo conteúdo",
                "overwrite": true
            }),
            authorization_ref: None,
        };
        let approval = broker.request_approval(&call).unwrap();
        assert_eq!(
            approval.summary.as_deref(),
            Some("sobrescrever /tmp/vox/README.md")
        );
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
    fn approval_is_bound_to_its_original_run() {
        let broker = Broker::default();
        let original = BrokerCall {
            call_id: "call-original".into(),
            run_id: "run-original".into(),
            tool: "shell.exec".into(),
            arguments: serde_json::json!({"program":"printf","argv":["ok"]}),
            authorization_ref: None,
        };
        let approval = broker.request_approval(&original).unwrap();
        let other_run = BrokerCall {
            call_id: "call-other".into(),
            run_id: "run-other".into(),
            tool: original.tool.clone(),
            arguments: original.arguments.clone(),
            authorization_ref: Some(approval.approval_id.clone()),
        };

        let rejected = broker.execute(&other_run).unwrap();
        assert_eq!(rejected.error_code.as_deref(), Some("INVALID_APPROVAL"));

        let authorized = BrokerCall {
            authorization_ref: Some(approval.approval_id),
            ..original
        };
        assert_eq!(broker.execute(&authorized).unwrap().status, "success");
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

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{atomic::AtomicBool, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;
use vox_desktop_access::{
    is_sensitive_semantic_node, query_snapshot, NativeDesktop, SemanticQuery, SemanticSnapshot,
};
use vox_policy::{PolicyEngine, PolicyError};
use vox_tool_runtime::{RuntimeConfig, ToolError, ToolRuntime};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrokerCall {
    pub call_id: String,
    pub run_id: String,
    pub task_grant_id: String,
    pub tool: String,
    pub arguments: Value,
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
    #[error("task grant is missing, expired, or belongs to another run")]
    TaskGrantUnavailable,
    #[error("the requested tool or target is outside the active task grant")]
    TaskGrantOutOfScope,
    #[error("the requested desktop field is sensitive and cannot be written")]
    SensitiveDesktopField,
}

const MAX_ISSUED_SNAPSHOTS: usize = 64;
const SNAPSHOT_TTL: Duration = Duration::from_secs(30);
const TASK_GRANT_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskGrant {
    pub id: String,
    pub run_id: String,
    pub request_hash: String,
    pub expires_at: u64,
    pub allowed_tools: BTreeSet<String>,
}

struct ActiveTaskGrant {
    grant: TaskGrant,
    created: Instant,
    observed_app_pids: BTreeSet<u32>,
    resolved_programs: BTreeSet<String>,
    observed_processes: BTreeMap<u32, (String, Option<u64>)>,
}

#[derive(Default)]
struct TaskGrantRegistry {
    grants: Mutex<BTreeMap<String, ActiveTaskGrant>>,
}

impl TaskGrantRegistry {
    fn begin(
        &self,
        run_id: &str,
        request_hash: &str,
        allowed_tools: impl IntoIterator<Item = String>,
    ) -> TaskGrant {
        let grant = TaskGrant {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: run_id.to_owned(),
            request_hash: request_hash.to_owned(),
            expires_at: std::time::SystemTime::now()
                .checked_add(TASK_GRANT_TTL)
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
                .unwrap_or_default(),
            allowed_tools: allowed_tools.into_iter().collect(),
        };
        if let Ok(mut grants) = self.grants.lock() {
            grants.retain(|_, active| active.created.elapsed() < TASK_GRANT_TTL);
            grants.insert(
                grant.id.clone(),
                ActiveTaskGrant {
                    grant: grant.clone(),
                    created: Instant::now(),
                    observed_app_pids: BTreeSet::new(),
                    resolved_programs: BTreeSet::new(),
                    observed_processes: BTreeMap::new(),
                },
            );
        }
        grant
    }

    fn active<'a>(
        grants: &'a mut BTreeMap<String, ActiveTaskGrant>,
        run_id: &str,
        grant_id: &str,
    ) -> Result<&'a mut ActiveTaskGrant, BrokerError> {
        let Some(active) = grants.get_mut(grant_id) else {
            return Err(BrokerError::TaskGrantUnavailable);
        };
        if active.grant.run_id != run_id || active.created.elapsed() >= TASK_GRANT_TTL {
            return Err(BrokerError::TaskGrantUnavailable);
        }
        Ok(active)
    }

    fn authorize(&self, run_id: &str, grant_id: &str, tool: &str) -> Result<(), BrokerError> {
        let mut grants = self
            .grants
            .lock()
            .map_err(|_| BrokerError::TaskGrantUnavailable)?;
        let active = Self::active(&mut grants, run_id, grant_id)?;
        if !active.grant.allowed_tools.contains(tool) {
            return Err(BrokerError::TaskGrantOutOfScope);
        }
        Ok(())
    }

    fn remember_windows(&self, run_id: &str, grant_id: &str, data: &Value) {
        let Ok(mut grants) = self.grants.lock() else {
            return;
        };
        let Ok(active) = Self::active(&mut grants, run_id, grant_id) else {
            return;
        };
        for pid in data
            .get("windows")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|window| window.get("app_pid").and_then(Value::as_u64))
            .filter_map(|pid| u32::try_from(pid).ok())
        {
            active.observed_app_pids.insert(pid);
        }
    }

    fn require_observed_app(
        &self,
        run_id: &str,
        grant_id: &str,
        pid: u32,
    ) -> Result<(), BrokerError> {
        let mut grants = self
            .grants
            .lock()
            .map_err(|_| BrokerError::TaskGrantUnavailable)?;
        if Self::active(&mut grants, run_id, grant_id)?
            .observed_app_pids
            .contains(&pid)
        {
            Ok(())
        } else {
            Err(BrokerError::TaskGrantOutOfScope)
        }
    }

    fn remember_resolved_program(&self, run_id: &str, grant_id: &str, data: &Value) {
        let Ok(mut grants) = self.grants.lock() else {
            return;
        };
        let Ok(active) = Self::active(&mut grants, run_id, grant_id) else {
            return;
        };
        if let Some(program) = data.get("program").and_then(Value::as_str) {
            active.resolved_programs.insert(program.to_owned());
        }
    }

    fn require_resolved_program(
        &self,
        run_id: &str,
        grant_id: &str,
        program: &str,
    ) -> Result<(), BrokerError> {
        let mut grants = self
            .grants
            .lock()
            .map_err(|_| BrokerError::TaskGrantUnavailable)?;
        if Self::active(&mut grants, run_id, grant_id)?
            .resolved_programs
            .contains(program)
        {
            Ok(())
        } else {
            Err(BrokerError::TaskGrantOutOfScope)
        }
    }

    fn remember_processes(&self, run_id: &str, grant_id: &str, data: &Value) {
        let Ok(mut grants) = self.grants.lock() else {
            return;
        };
        let Ok(active) = Self::active(&mut grants, run_id, grant_id) else {
            return;
        };
        for process in data
            .get("processes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(pid) = process
                .get("pid")
                .and_then(Value::as_u64)
                .and_then(|pid| u32::try_from(pid).ok())
            else {
                continue;
            };
            let Some(command) = process.get("command").and_then(Value::as_str) else {
                continue;
            };
            active.observed_processes.insert(
                pid,
                (
                    command.to_owned(),
                    process.get("start_time").and_then(Value::as_u64),
                ),
            );
        }
    }

    fn require_observed_process(
        &self,
        run_id: &str,
        grant_id: &str,
        pid: u32,
        command: Option<&str>,
        start_time: Option<u64>,
    ) -> Result<(), BrokerError> {
        let mut grants = self
            .grants
            .lock()
            .map_err(|_| BrokerError::TaskGrantUnavailable)?;
        let Some((observed_command, observed_start)) = Self::active(&mut grants, run_id, grant_id)?
            .observed_processes
            .get(&pid)
        else {
            return Err(BrokerError::TaskGrantOutOfScope);
        };
        if command == Some(observed_command.as_str()) && start_time == *observed_start {
            Ok(())
        } else {
            Err(BrokerError::TaskGrantOutOfScope)
        }
    }

    fn finish(&self, grant_id: &str) {
        if let Ok(mut grants) = self.grants.lock() {
            grants.remove(grant_id);
        }
    }
}

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
    grants: TaskGrantRegistry,
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
            grants: TaskGrantRegistry::default(),
        }
    }

    /// The desktop creates this grant only after the user submits a task.
    /// It is invalidated at the terminal run state.
    pub fn begin_task_grant(
        &self,
        run_id: &str,
        request_hash: &str,
        allowed_tools: impl IntoIterator<Item = String>,
    ) -> TaskGrant {
        self.grants.begin(run_id, request_hash, allowed_tools)
    }

    pub fn finish_task_grant(&self, grant_id: &str) {
        self.grants.finish(grant_id);
    }

    pub fn execute(&self, call: &BrokerCall) -> Result<BrokerResult, BrokerError> {
        self.execute_with_cancel(call, &AtomicBool::new(false))
    }

    pub fn execute_with_cancel(
        &self,
        call: &BrokerCall,
        cancel: &AtomicBool,
    ) -> Result<BrokerResult, BrokerError> {
        self.grants
            .authorize(&call.run_id, &call.task_grant_id, &call.tool)?;
        self.policy.validate(&call.tool, &call.arguments)?;
        let result = match call.tool.as_str() {
            "desktop.list_windows" => {
                let result = self.runtime.list_windows()?;
                self.grants
                    .remember_windows(&call.run_id, &call.task_grant_id, &result.data);
                result
            }
            "desktop.snapshot" => {
                let object = call
                    .arguments
                    .as_object()
                    .ok_or_else(|| BrokerError::ArgumentsNotObject(call.tool.clone()))?;
                let app_pid = optional_desktop_app_pid(object, &call.tool)?;
                if let Some(pid) = app_pid {
                    self.grants
                        .require_observed_app(&call.run_id, &call.task_grant_id, pid)?;
                }
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
                if matches!(action, "set_value" | "set-value")
                    && snapshot
                        .nodes
                        .iter()
                        .find(|node| node.element_ref == element_ref)
                        .is_some_and(is_sensitive_semantic_node)
                {
                    return Err(BrokerError::SensitiveDesktopField);
                }
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
                let result = self.runtime.list_processes(limit)?;
                self.grants
                    .remember_processes(&call.run_id, &call.task_grant_id, &result.data);
                result
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
                self.grants.require_observed_process(
                    &call.run_id,
                    &call.task_grant_id,
                    pid,
                    expected,
                    start_time,
                )?;
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
                self.grants
                    .require_resolved_program(&call.run_id, &call.task_grant_id, program)?;
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
                let result = self.runtime.resolve_app(name)?;
                self.grants.remember_resolved_program(
                    &call.run_id,
                    &call.task_grant_id,
                    &result.data,
                );
                result
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
}

#[cfg(test)]
mod task_grant_tests {
    use super::*;

    #[test]
    fn grant_is_bound_to_its_run_and_cannot_be_replayed() {
        let broker = Broker::default();
        let grant = broker.begin_task_grant("run-a", "request-hash", ["process.list".to_owned()]);
        let call = BrokerCall {
            call_id: "call-a".into(),
            run_id: "run-b".into(),
            task_grant_id: grant.id.clone(),
            tool: "process.list".into(),
            arguments: serde_json::json!({"limit": 1}),
        };
        assert!(matches!(
            broker.execute(&call),
            Err(BrokerError::TaskGrantUnavailable)
        ));
        broker.finish_task_grant(&grant.id);
        let call = BrokerCall {
            run_id: "run-a".into(),
            ..call
        };
        assert!(matches!(
            broker.execute(&call),
            Err(BrokerError::TaskGrantUnavailable)
        ));
    }

    #[test]
    fn active_grant_executes_registered_read_tools_without_a_modal() {
        let broker = Broker::default();
        let grant = broker.begin_task_grant("run-a", "request-hash", ["process.list".to_owned()]);
        let call = BrokerCall {
            call_id: "call-a".into(),
            run_id: "run-a".into(),
            task_grant_id: grant.id,
            tool: "process.list".into(),
            arguments: serde_json::json!({"limit": 1}),
        };
        assert_eq!(broker.execute(&call).unwrap().status, "success");
    }

    #[test]
    fn grant_rejects_a_tool_outside_the_explicit_task_scope() {
        let broker = Broker::default();
        let grant = broker.begin_task_grant("run-a", "request-hash", ["process.list".to_owned()]);
        let call = BrokerCall {
            call_id: "call-a".into(),
            run_id: "run-a".into(),
            task_grant_id: grant.id,
            tool: "shell.exec".into(),
            arguments: serde_json::json!({"program":"true"}),
        };
        assert!(matches!(
            broker.execute(&call),
            Err(BrokerError::TaskGrantOutOfScope)
        ));
    }
}

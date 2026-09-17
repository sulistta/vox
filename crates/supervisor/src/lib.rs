use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use vox_protocol::{encode, validate_line, ProtocolError};

#[cfg(unix)]
fn isolate_core_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn isolate_core_process_group(_command: &mut Command) {}

fn terminate_core(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("failed to spawn core: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("core stdin is unavailable")]
    NoStdin,
    #[error("core message is invalid: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("core writer is poisoned")]
    WriterPoisoned,
}

pub struct AgentSupervisor {
    writer: Arc<Mutex<ChildStdin>>,
    events: Receiver<Result<Value, ProtocolError>>,
    child: Option<Child>,
    shutdown: Sender<()>,
    stopping: Arc<AtomicBool>,
    pending: Arc<Mutex<BTreeMap<String, Instant>>>,
    run_requests: Arc<Mutex<BTreeMap<String, String>>>,
    last_activity: Arc<Mutex<Instant>>,
}

impl AgentSupervisor {
    pub fn spawn(
        node: impl AsRef<std::path::Path>,
        entry: impl AsRef<std::path::Path>,
    ) -> Result<Self, SupervisorError> {
        Self::spawn_with_env(node, entry, &[])
    }

    pub fn spawn_with_env(
        node: impl AsRef<std::path::Path>,
        entry: impl AsRef<std::path::Path>,
        environment: &[(String, String)],
    ) -> Result<Self, SupervisorError> {
        let mut command = Command::new(node.as_ref());
        command
            .arg(entry.as_ref())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in environment {
            command.env(key, value);
        }
        isolate_core_process_group(&mut command);
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or(SupervisorError::NoStdin)?;
        let stdout = child.stdout.take().ok_or(SupervisorError::NoStdin)?;
        let stderr = child.stderr.take().ok_or(SupervisorError::NoStdin)?;
        let (events_tx, events_rx) = unbounded();
        let (shutdown_tx, shutdown_rx) = unbounded();
        let stopping = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let run_requests = Arc::new(Mutex::new(BTreeMap::new()));
        let last_activity = Arc::new(Mutex::new(Instant::now()));
        let reader_stopping = Arc::clone(&stopping);
        let reader_pending = Arc::clone(&pending);
        let reader_run_requests = Arc::clone(&run_requests);
        let reader_last_activity = Arc::clone(&last_activity);
        thread::Builder::new()
            .name("vox-core-stdout".into())
            .spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    match line {
                        Ok(line) if !line.trim().is_empty() => {
                            let parsed = validate_line(&line);
                            if let Ok(value) = &parsed {
                                if let Ok(mut last) = reader_last_activity.lock() {
                                    *last = Instant::now();
                                }
                                if let Some(request_id) =
                                    value.get("request_id").and_then(Value::as_str)
                                {
                                    if let Ok(mut requests) = reader_pending.lock() {
                                        requests.remove(request_id);
                                    }
                                }
                                let terminal = matches!(
                                    value.get("type").and_then(Value::as_str),
                                    Some("run.completed" | "run.failed" | "run.cancelled")
                                );
                                if terminal {
                                    if let Some(run_id) =
                                        value.get("run_id").and_then(Value::as_str)
                                    {
                                        if let Ok(mut runs) = reader_run_requests.lock() {
                                            if let Some(request_id) = runs.remove(run_id) {
                                                if let Ok(mut requests) = reader_pending.lock() {
                                                    requests.remove(&request_id);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            let _ = events_tx.send(parsed);
                        }
                        Ok(_) => {}
                        Err(error) => {
                            let _ =
                                events_tx.send(Err(ProtocolError::InvalidJson(error.to_string())));
                            break;
                        }
                    }
                }
                if !reader_stopping.load(Ordering::Relaxed) {
                    let _ = events_tx.send(Err(ProtocolError::InvalidJson(
                        "core stdout closed unexpectedly".into(),
                    )));
                }
            })
            .ok();
        thread::Builder::new()
            .name("vox-core-stderr".into())
            .spawn(move || {
                let reader = BufReader::new(stderr);
                let mut reported = false;
                for line in reader.lines() {
                    if shutdown_rx.try_recv().is_ok() {
                        break;
                    }
                    if line.is_ok() && !reported {
                        // A provider or an unexpected dependency can echo a
                        // request on stderr. The structured protocol carries
                        // the user-safe error state, so never mirror raw
                        // diagnostics into the desktop process log.
                        eprintln!("[vox-agent] diagnóstico do core suprimido para proteger conteúdo sensível");
                        reported = true;
                    }
                }
            })
            .ok();
        Ok(Self {
            writer: Arc::new(Mutex::new(stdin)),
            events: events_rx,
            child: Some(child),
            shutdown: shutdown_tx,
            stopping,
            pending,
            run_requests,
            last_activity,
        })
    }

    pub fn send(&self, value: &Value) -> Result<(), SupervisorError> {
        let line = encode(value)?;
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| SupervisorError::WriterPoisoned)?;
        writer.write_all(line.as_bytes())?;
        writer.flush()?;
        if value.get("type").and_then(Value::as_str) != Some("turn.cancel") {
            if let Some(request_id) = value.get("request_id").and_then(Value::as_str) {
                if let Ok(mut requests) = self.pending.lock() {
                    requests.insert(request_id.to_owned(), Instant::now());
                }
                if value.get("type").and_then(Value::as_str) == Some("turn.start") {
                    if let Some(run_id) = value.get("run_id").and_then(Value::as_str) {
                        if let Ok(mut runs) = self.run_requests.lock() {
                            runs.insert(run_id.to_owned(), request_id.to_owned());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn initialize(&self) -> Result<(), SupervisorError> {
        self.send(&json!({"type":"initialize","protocol":1,"build":"vox-desktop"}))
    }

    pub fn open_session(&self, request_id: &str, session_id: &str) -> Result<(), SupervisorError> {
        self.send(&json!({"type":"session.open","request_id":request_id,"session_id":session_id}))
    }

    pub fn start_turn(
        &self,
        request_id: &str,
        session_id: &str,
        run_id: &str,
        content: &str,
    ) -> Result<(), SupervisorError> {
        self.start_turn_with_context(request_id, session_id, run_id, content, "text", &[])
    }

    /// Start a turn with a bounded, already-redacted conversation context.
    /// The supervisor only transports it; construction and authorization stay
    /// with the desktop/session layers and the core validates the final IPC
    /// shape before using it.
    pub fn start_turn_with_context(
        &self,
        request_id: &str,
        session_id: &str,
        run_id: &str,
        content: &str,
        source: &str,
        context: &[Value],
    ) -> Result<(), SupervisorError> {
        self.send(&json!({
            "type":"turn.start",
            "request_id":request_id,
            "session_id":session_id,
            "run_id":run_id,
            "content":content,
            "source":source,
            "context":context,
        }))
    }

    pub fn cancel(&self, request_id: &str, run_id: &str) -> Result<(), SupervisorError> {
        self.send(&json!({"type":"turn.cancel","request_id":request_id,"run_id":run_id}))
    }

    pub fn heartbeat(&self, request_id: &str) -> Result<(), SupervisorError> {
        self.send(&json!({"type":"heartbeat","request_id":request_id}))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn send_tool_result(
        &self,
        run_id: &str,
        call_id: &str,
        tool: &str,
        status: &str,
        side_effect: &str,
        data: &Value,
        error: Option<&str>,
    ) -> Result<(), SupervisorError> {
        self.send_tool_result_detailed(
            run_id,
            call_id,
            tool,
            status,
            side_effect,
            data,
            None,
            error,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn send_tool_result_detailed(
        &self,
        run_id: &str,
        call_id: &str,
        tool: &str,
        status: &str,
        side_effect: &str,
        data: &Value,
        error_code: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), SupervisorError> {
        let observed = status == "success" && side_effect != "unknown";
        self.send_tool_result_with_verification(
            run_id,
            call_id,
            tool,
            status,
            side_effect,
            data,
            Some(&json!({"observed": observed})),
            error_code,
            error,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn send_tool_result_with_verification(
        &self,
        run_id: &str,
        call_id: &str,
        tool: &str,
        status: &str,
        side_effect: &str,
        data: &Value,
        verification: Option<&Value>,
        error_code: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), SupervisorError> {
        self.send(&json!({
            "type":"tool.result",
            "run_id":run_id,
            "call_id":call_id,
            "tool":tool,
            "status":status,
            "side_effect":side_effect,
            "data":data,
            "error_code":error_code,
            "error":error,
            "verification":verification.cloned().unwrap_or_else(|| json!({"observed":false}))
        }))
    }

    pub fn try_event(&self) -> Option<Result<Value, ProtocolError>> {
        match self.events.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }

    pub fn pending_requests(&self) -> Vec<(String, Duration)> {
        let now = Instant::now();
        self.pending
            .lock()
            .map(|requests| {
                requests
                    .iter()
                    .map(|(id, started)| (id.clone(), now.saturating_duration_since(*started)))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn idle_for(&self) -> Duration {
        self.last_activity
            .lock()
            .map(|last| Instant::now().saturating_duration_since(*last))
            .unwrap_or_default()
    }

    pub fn expire_pending(&self, timeout: Duration) -> Vec<String> {
        let now = Instant::now();
        let mut expired = Vec::new();
        if let Ok(mut requests) = self.pending.lock() {
            requests.retain(|id, started| {
                let keep = now.saturating_duration_since(*started) < timeout;
                if !keep {
                    expired.push(id.clone());
                }
                keep
            });
        }
        expired
    }

    /// Return and remove requests that exceeded the watchdog deadline. The
    /// caller still decides how to reconcile the associated run; expiring a
    /// bookkeeping entry must never be reported as a successful action.
    pub fn watchdog_expired(&self, timeout: Duration) -> Vec<String> {
        self.expire_pending(timeout)
    }
}

impl Drop for AgentSupervisor {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        let _ = self.shutdown.send(());
        if let Some(mut child) = self.child.take() {
            terminate_core(&mut child);
        }
    }
}

pub fn default_entry() -> std::path::PathBuf {
    std::env::var_os("VOX_AGENT_ENTRY")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("packages/agent-core/dist/main.js"))
}

pub fn default_node() -> std::path::PathBuf {
    std::env::var_os("VOX_NODE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("node"))
}

pub fn poll_with_timeout(
    supervisor: &AgentSupervisor,
    timeout: Duration,
) -> Option<Result<Value, ProtocolError>> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(event) = supervisor.try_event() {
            return Some(event);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

use std::path::PathBuf;
use std::time::{Duration, Instant};
use vox_supervisor::{poll_with_timeout, AgentSupervisor};

#[test]
fn private_core_handshake_and_turn_are_structured() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let entry = std::env::var_os("VOX_AGENT_ENTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("packages/agent-core/dist/main.js"));
    if !entry.exists() {
        panic!(
            "build the TypeScript core before this integration test: {}",
            entry.display()
        );
    }
    let supervisor = AgentSupervisor::spawn("node", &entry).expect("spawn private core");
    supervisor.initialize().expect("initialize");
    let initialized = poll_with_timeout(&supervisor, Duration::from_secs(2))
        .expect("initialized event")
        .expect("valid initialized event");
    assert_eq!(initialized["type"], "initialized");
    supervisor.heartbeat("heartbeat-test").expect("heartbeat");
    let heartbeat = poll_with_timeout(&supervisor, Duration::from_secs(2))
        .expect("heartbeat event")
        .expect("valid heartbeat event");
    assert_eq!(heartbeat["type"], "heartbeat");
    supervisor
        .open_session("open", "session-test")
        .expect("open session");
    let opened = poll_with_timeout(&supervisor, Duration::from_secs(2))
        .expect("session event")
        .expect("valid session event");
    assert_eq!(opened["type"], "session.opened");
    assert!(supervisor.pending_requests().is_empty());
    supervisor
        .start_turn("turn", "session-test", "run-test", "oi")
        .expect("start turn");
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_delta = false;
    let mut saw_completed = false;
    while Instant::now() < deadline {
        if let Some(event) = supervisor.try_event() {
            let event = event.expect("valid core event");
            match event["type"].as_str() {
                Some("message.delta") => saw_delta = true,
                Some("run.completed") => {
                    saw_completed = true;
                    break;
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(saw_delta, "streaming delta was not observed");
    assert!(saw_completed, "terminal event was not observed");
    assert!(supervisor.pending_requests().is_empty());
}

#[test]
fn a09_wrong_call_id_cannot_complete_a_pending_tool() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let entry = std::env::var_os("VOX_AGENT_ENTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("packages/agent-core/dist/main.js"));
    if !entry.exists() {
        panic!(
            "build the TypeScript core before this integration test: {}",
            entry.display()
        );
    }
    let supervisor = AgentSupervisor::spawn("node", &entry).expect("spawn private core");
    supervisor.initialize().expect("initialize");
    let _ = poll_with_timeout(&supervisor, Duration::from_secs(2)).expect("initialized");
    supervisor
        .open_session("open-tool", "session-tool")
        .unwrap();
    let _ = poll_with_timeout(&supervisor, Duration::from_secs(2)).expect("opened");
    supervisor
        .start_turn("turn-tool", "session-tool", "run-tool", "mostrar janelas")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut call_id = None;
    let mut saw_tool_completed = false;
    let mut saw_run_completed = false;
    while Instant::now() < deadline {
        if let Some(event) = supervisor.try_event() {
            let event = event.expect("valid core event");
            match event["type"].as_str() {
                Some("tool.execute.requested") => {
                    let id = event["call_id"].as_str().unwrap().to_string();
                    call_id = Some(id.clone());
                    supervisor
                        .send_tool_result(
                            "run-tool",
                            "wrong-call-id",
                            "desktop.list_windows",
                            "success",
                            "none",
                            &serde_json::json!({"windows": []}),
                            None,
                        )
                        .unwrap();
                    let _ = poll_with_timeout(&supervisor, Duration::from_secs(1));
                    supervisor
                        .send_tool_result(
                            "run-tool",
                            &id,
                            "desktop.list_windows",
                            "success",
                            "none",
                            &serde_json::json!({"windows": [], "count": 0}),
                            None,
                        )
                        .unwrap();
                }
                Some("tool.completed") => saw_tool_completed = true,
                Some("run.completed") => {
                    saw_run_completed = true;
                    break;
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(call_id.is_some(), "tool request was not observed");
    assert!(saw_tool_completed, "tool completion was not observed");
    assert!(saw_run_completed, "run completion was not observed");
}

#[test]
fn unexpected_core_exit_is_reported_without_hanging_the_caller() {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/exit.mjs");
    let supervisor = AgentSupervisor::spawn("node", &entry).expect("spawn crashing core");
    let event = poll_with_timeout(&supervisor, Duration::from_secs(2))
        .expect("core exit diagnostic")
        .expect_err("unexpected stdout close must be surfaced as a protocol diagnostic");
    assert!(event.to_string().contains("stdout closed unexpectedly"));
}

#[test]
fn invalid_core_stdout_is_reported_as_a_protocol_diagnostic() {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/invalid.mjs");
    let supervisor = AgentSupervisor::spawn("node", &entry).expect("spawn invalid core");
    let event = poll_with_timeout(&supervisor, Duration::from_secs(2))
        .expect("invalid stdout diagnostic")
        .expect_err("invalid core stdout must not be accepted");
    assert!(event.to_string().contains("invalid JSON"));
}

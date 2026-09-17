use std::path::PathBuf;
use std::time::{Duration, Instant};
use vox_supervisor::{poll_with_timeout, AgentSupervisor};

#[test]
fn heartbeat_activity_keeps_a_slow_turn_alive() {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/slow-turn.mjs");
    let supervisor = AgentSupervisor::spawn("node", &entry).expect("spawn slow core fixture");

    supervisor.initialize().expect("initialize");
    assert_eq!(
        poll_with_timeout(&supervisor, Duration::from_secs(1))
            .expect("initialized event")
            .expect("valid initialized event")["type"],
        "initialized"
    );
    supervisor
        .open_session("open-slow", "slow-session")
        .expect("open session");
    assert_eq!(
        poll_with_timeout(&supervisor, Duration::from_secs(1))
            .expect("session opened")
            .expect("valid session event")["type"],
        "session.opened"
    );
    supervisor
        .start_turn("slow-turn", "slow-session", "slow-run", "responda devagar")
        .expect("start slow turn");

    // This is deliberately shorter than the fixture's 320 ms provider wait.
    // A desktop watchdog must use continued core activity (the heartbeat
    // replies below), not the age of the original turn.start request.
    let unresponsive_after = Duration::from_millis(125);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut next_heartbeat = Instant::now();
    let mut heartbeat_number = 0_u32;
    let mut saw_thinking = false;
    let mut saw_heartbeat = false;
    let mut saw_completion = false;

    while Instant::now() < deadline && !saw_completion {
        if Instant::now() >= next_heartbeat {
            supervisor
                .heartbeat(&format!("slow-heartbeat-{heartbeat_number}"))
                .expect("send heartbeat during slow turn");
            heartbeat_number += 1;
            next_heartbeat += Duration::from_millis(25);
        }

        while let Some(event) = supervisor.try_event() {
            let event = event.expect("valid fixture event");
            match event["type"].as_str() {
                Some("state.changed") => saw_thinking = true,
                Some("heartbeat") => saw_heartbeat = true,
                Some("run.completed") => saw_completion = true,
                _ => {}
            }
        }

        assert!(
            supervisor.idle_for() < unresponsive_after,
            "a core that answers heartbeats during a slow turn must not be considered dead"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    assert!(saw_thinking, "slow turn did not enter thinking state");
    assert!(saw_heartbeat, "slow core did not answer a heartbeat");
    assert!(saw_completion, "slow turn did not complete");
}

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
fn context_and_redacted_model_activity_cross_the_supervisor_boundary() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let entry = std::env::var_os("VOX_AGENT_ENTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("packages/agent-core/dist/main.js"));
    assert!(entry.exists(), "build the TypeScript core before this test");
    let supervisor = AgentSupervisor::spawn("node", &entry).expect("spawn private core");
    supervisor.initialize().expect("initialize");
    let _ = poll_with_timeout(&supervisor, Duration::from_secs(2)).expect("initialized");
    supervisor
        .open_session("open-context", "session-context")
        .expect("open session");
    let _ = poll_with_timeout(&supervisor, Duration::from_secs(2)).expect("opened");
    supervisor
        .start_turn_with_context(
            "turn-context",
            "session-context",
            "run-context",
            "pedido atual Bearer current-secret",
            "text",
            &[serde_json::json!({
                "role":"assistant",
                "content":"histórico API_KEY=context-secret"
            })],
        )
        .expect("start contextual turn");

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_activity = false;
    let mut completed = false;
    while Instant::now() < deadline {
        if let Some(event) = supervisor.try_event() {
            let event = event.expect("valid core event");
            match event["type"].as_str() {
                Some("model.requested") => {
                    saw_activity = true;
                    assert_eq!(event["redacted"], true);
                    let rendered = event["messages"].to_string();
                    assert!(rendered.contains("histórico"));
                    assert!(rendered.contains("[REDACTED]"));
                    assert!(!rendered.contains("context-secret"));
                    assert!(!rendered.contains("current-secret"));
                }
                Some("run.completed") => {
                    completed = true;
                    break;
                }
                Some("run.failed") => panic!("core failed: {event}"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(saw_activity, "model activity was not observed");
    assert!(completed, "contextual turn did not complete");
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
    let supervisor = AgentSupervisor::spawn_with_env(
        "node",
        &entry,
        &[("VOX_PROVIDER".into(), "fake-tools".into())],
    )
    .expect("spawn private core");
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

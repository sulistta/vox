use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vox_broker::{Broker, BrokerCall};
use vox_supervisor::{poll_with_timeout, AgentSupervisor};
use vox_tool_runtime::RuntimeConfig;

fn fixture_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "vox-e2e-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[cfg(unix)]
fn executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn start_supervisor() -> AgentSupervisor {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let entry = std::env::var_os("VOX_AGENT_ENTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("packages/agent-core/dist/main.js"));
    assert!(entry.exists(), "build the TypeScript core before E2E tests");
    let supervisor = AgentSupervisor::spawn_with_env(
        std::env::var_os("VOX_NODE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("node")),
        entry,
        &[],
    )
    .expect("spawn private core");
    supervisor.initialize().expect("initialize core");
    let initialized = poll_with_timeout(&supervisor, Duration::from_secs(2))
        .expect("initialized event")
        .expect("valid initialized event");
    assert_eq!(initialized["type"], "initialized");
    supervisor
}

fn wait_for_turn(
    supervisor: &AgentSupervisor,
    broker: &Broker,
    session_id: &str,
    run_id: &str,
    content: &str,
) -> (bool, bool) {
    supervisor
        .open_session("e2e-open", session_id)
        .expect("open session");
    assert_eq!(
        poll_with_timeout(supervisor, Duration::from_secs(2))
            .expect("session opened")
            .expect("valid session event")["type"],
        "session.opened"
    );
    let grant = broker.begin_task_grant(
        run_id,
        "e2e-request",
        ["clipboard.read".to_owned(), "clipboard.write".to_owned()],
    );
    supervisor
        .start_turn_with_context(
            "e2e-turn",
            session_id,
            run_id,
            &grant.id,
            content,
            "text",
            &[],
        )
        .expect("start turn");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_tool = false;
    let mut saw_completion = false;
    while Instant::now() < deadline {
        if let Some(event) = supervisor.try_event() {
            let event = event.expect("valid core event");
            match event["type"].as_str() {
                Some("tool.execute.requested") => {
                    saw_tool = true;
                    let call = BrokerCall {
                        call_id: event["call_id"].as_str().unwrap().into(),
                        run_id: event["run_id"].as_str().unwrap().into(),
                        task_grant_id: event["task_grant_id"].as_str().unwrap().into(),
                        tool: event["tool"].as_str().unwrap().into(),
                        arguments: event["arguments"].clone(),
                    };
                    let result = broker.execute(&call).expect("execute task-granted call");
                    supervisor
                        .send_tool_result_with_verification(
                            &call.run_id,
                            &call.call_id,
                            &call.tool,
                            &result.status,
                            &result.side_effect,
                            &result.data,
                            result.verification.as_ref(),
                            result.error_code.as_deref(),
                            result.error.as_deref(),
                        )
                        .expect("send broker result");
                }
                Some("run.completed") => {
                    saw_completion = true;
                    break;
                }
                Some("run.failed") => panic!("core run failed: {event}"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    (saw_tool, saw_completion)
}

#[cfg(unix)]
#[test]
#[ignore = "requires configured Jev and text services"]
fn e2e_core_supervisor_broker_and_clipboard_read_cross_the_ipc_boundary() {
    let root = fixture_root();
    fs::create_dir_all(&root).unwrap();
    let read_script = root.join("read.sh");
    executable(&read_script, "#!/bin/sh\nprintf 'e2e clipboard text'\n");
    let broker = Broker::new(RuntimeConfig {
        allowed_roots: vec![root.clone()],
        native_access: false,
        clipboard_read_command: Some(read_script),
        ..RuntimeConfig::default()
    });
    let supervisor = start_supervisor();
    let (saw_tool, saw_completion) = wait_for_turn(
        &supervisor,
        &broker,
        "e2e-read-session",
        "e2e-read-run",
        "leia o clipboard",
    );
    assert!(saw_tool);
    assert!(saw_completion);
    drop(supervisor);
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
#[ignore = "requires configured Jev and text services"]
fn e2e_core_supervisor_broker_and_task_grant_cross_the_ipc_boundary() {
    let root = fixture_root();
    fs::create_dir_all(&root).unwrap();
    let write_script = root.join("write.sh");
    let output = root.join("written.txt");
    executable(
        &write_script,
        &format!("#!/bin/sh\ncat > '{}'\n", output.display()),
    );
    let broker = Broker::new(RuntimeConfig {
        allowed_roots: vec![root.clone()],
        native_access: false,
        clipboard_write_command: Some(write_script),
        ..RuntimeConfig::default()
    });
    let supervisor = start_supervisor();
    let (saw_tool, saw_completion) = wait_for_turn(
        &supervisor,
        &broker,
        "e2e-write-session",
        "e2e-write-run",
        "copie este texto para a área de transferência",
    );
    assert!(saw_tool);
    assert!(saw_completion);
    let written = fs::read_to_string(&output).expect("approved write fixture output");
    assert!(written.contains("copie este texto"));
    drop(supervisor);
    let _ = fs::remove_dir_all(root);
}

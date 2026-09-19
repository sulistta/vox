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

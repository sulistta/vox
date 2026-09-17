//! Opt-in real-session test for the native AT-SPI bridge.
//!
//! Run with:
//! `cargo test -p vox-desktop-access --test native_fixture -- --ignored --nocapture`
//!
//! It starts only the controlled GTK fixture in `tests/fixtures`, never an
//! arbitrary desktop application. CI and headless environments skip it.

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vox_desktop_access::{
    query_snapshot, NativeDesktop, NativeSnapshotLimits, SemanticQuery, SemanticSnapshot,
};

struct FixtureProcess {
    child: Child,
    ready_file: PathBuf,
}

impl Drop for FixtureProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.ready_file);
    }
}

fn fixture_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/accessibility_fixture.py")
}

fn temporary_ready_file() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "vox-accessibility-fixture-{}-{nonce}.ready",
        std::process::id()
    ))
}

fn start_fixture() -> FixtureProcess {
    let ready_file = temporary_ready_file();
    let mut child = Command::new("python3")
        .arg(fixture_script())
        .arg("--ready-file")
        .arg(&ready_file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("GTK accessibility fixture should start");
    let deadline = Instant::now() + Duration::from_secs(8);
    while !ready_file.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    if !ready_file.exists() {
        let _ = child.kill();
        let _ = child.wait();
        panic!("fixture did not signal readiness");
    }
    FixtureProcess { child, ready_file }
}

fn wait_for_snapshot(
    desktop: &NativeDesktop,
    app_pid: Option<u32>,
    name: &str,
) -> SemanticSnapshot {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(snapshots) = desktop.snapshots_for_app_pid(app_pid) {
            if let Some(snapshot) = snapshots
                .into_iter()
                .find(|snapshot| snapshot.nodes.iter().any(|node| node.name == name))
            {
                return snapshot;
            }
        }
        assert!(
            Instant::now() < deadline,
            "fixture element {name:?} was not observed"
        );
        thread::sleep(Duration::from_millis(75));
    }
}

#[test]
#[ignore = "requires a live Linux AT-SPI session"]
fn controlled_gtk_fixture_covers_adversarial_native_accessibility_edges() {
    let _fixture = start_fixture();
    let desktop = NativeDesktop::default();
    let snapshot = wait_for_snapshot(&desktop, None, "Vox Accessibility Fixture");
    let fixture_pid = snapshot
        .app_id
        .rsplit(':')
        .next()
        .and_then(|pid| pid.parse::<u32>().ok())
        .expect("fixture app id should retain its PID");
    let targeted = desktop
        .snapshots_for_app_pid(Some(fixture_pid))
        .expect("targeted fixture observation should remain available");
    assert!(targeted
        .iter()
        .all(|candidate| candidate.app_id == snapshot.app_id));
    assert!(targeted.iter().any(|candidate| candidate
        .nodes
        .iter()
        .any(|node| node.name == "Safe action")));

    let duplicate_actions = query_snapshot(
        &snapshot,
        &SemanticQuery {
            name: Some("Duplicate action".into()),
            action: Some("press".into()),
            ..Default::default()
        },
    );
    assert_eq!(
        duplicate_actions.element_refs.len(),
        2,
        "native duplicate names must remain distinguishable and ambiguous"
    );

    for (label, replacement) in [("Senha", "new-secret"), ("Token de acesso", "new-token")] {
        let sensitive = snapshot
            .nodes
            .iter()
            .find(|node| node.name == label)
            .unwrap_or_else(|| panic!("fixture field {label:?} should be observable"));
        assert_eq!(sensitive.value.as_deref(), Some("[REDACTED]"));
        assert!(
            desktop
                .act(
                    &snapshot,
                    &sensitive.element_ref,
                    "set_value",
                    Some(replacement)
                )
                .is_err(),
            "native field {label:?} must reject model-driven writes"
        );
    }

    let safe_action = snapshot
        .nodes
        .iter()
        .find(|node| {
            node.name == "Safe action" && node.actions.iter().any(|action| action == "press")
        })
        .expect("fixture safe action should be pressable")
        .element_ref
        .clone();
    let result = desktop
        .act(&snapshot, &safe_action, "press", None)
        .expect("fixture safe action should resolve after a fresh observation");
    assert_eq!(result["action"], "press");
    assert_eq!(result["verified"], false);
    assert_eq!(result["verification"]["postcondition"], "unknown");

    let observed = desktop
        .wait_for_with_cancel_for_app_pid(
            &SemanticQuery {
                name: Some("Action count: 1".into()),
                ..Default::default()
            },
            Duration::from_secs(3),
            &AtomicBool::new(false),
            Some(fixture_pid),
        )
        .expect("accessibility wait should remain available");
    assert!(!observed.element_refs.is_empty());

    let snapshot = wait_for_snapshot(&desktop, Some(fixture_pid), "Ephemeral action 0");
    let old_ephemeral_ref = snapshot
        .nodes
        .iter()
        .find(|node| {
            node.name == "Ephemeral action 0" && node.actions.iter().any(|action| action == "press")
        })
        .expect("fixture page-one action should be pressable")
        .element_ref
        .clone();
    let recreate = snapshot
        .nodes
        .iter()
        .find(|node| {
            node.name == "Recreate result" && node.actions.iter().any(|action| action == "press")
        })
        .expect("fixture list trigger should be pressable")
        .element_ref
        .clone();
    desktop
        .act(&snapshot, &recreate, "press", None)
        .expect("fixture list trigger should resolve after a fresh observation");
    let recreated = wait_for_snapshot(&desktop, Some(fixture_pid), "Recreated result 1");
    assert!(recreated
        .nodes
        .iter()
        .any(|node| node.name == "Recreated result 1"));
    let new_ephemeral_ref = recreated
        .nodes
        .iter()
        .find(|node| node.name == "Ephemeral action 1")
        .expect("fixture page-two action should be freshly observed")
        .element_ref
        .clone();
    assert_ne!(old_ephemeral_ref, new_ephemeral_ref);
    assert!(
        desktop
            .act(&snapshot, &old_ephemeral_ref, "press", None)
            .is_err(),
        "a stale native reference must not dispatch to the replacement node"
    );

    let limited = NativeDesktop {
        limits: NativeSnapshotLimits {
            max_nodes: 4,
            max_depth: 8,
        },
    };
    assert!(
        limited
            .snapshots_for_app_pid(Some(fixture_pid))
            .expect("limited native observation should still return a snapshot")
            .iter()
            .any(|candidate| candidate.truncated),
        "the native adapter must declare a capped observation rather than silently hiding nodes"
    );

    let snapshot = wait_for_snapshot(&desktop, Some(fixture_pid), "Open confirmation");
    let open_confirmation = snapshot
        .nodes
        .iter()
        .find(|node| {
            node.name == "Open confirmation" && node.actions.iter().any(|action| action == "press")
        })
        .expect("fixture dialog trigger should be pressable")
        .element_ref
        .clone();
    desktop
        .act(&snapshot, &open_confirmation, "press", None)
        .expect("fixture dialog trigger should resolve after a fresh observation");
    let dialog_state = wait_for_snapshot(&desktop, Some(fixture_pid), "Dialog state: open");
    assert!(dialog_state
        .nodes
        .iter()
        .any(|node| node.name == "Dialog state: open"));
    let dialog = wait_for_snapshot(&desktop, Some(fixture_pid), "Confirm");
    let confirm = dialog
        .nodes
        .iter()
        .find(|node| node.name == "Confirm" && node.actions.iter().any(|action| action == "press"))
        .expect("fixture confirmation dialog should expose a pressable confirm action")
        .element_ref
        .clone();
    desktop
        .act(&dialog, &confirm, "press", None)
        .expect("fixture confirmation action should resolve after a fresh observation");
    let confirmed = wait_for_snapshot(&desktop, Some(fixture_pid), "Dialog state: confirmed");
    assert!(confirmed
        .nodes
        .iter()
        .any(|node| node.name == "Dialog state: confirmed"));
}

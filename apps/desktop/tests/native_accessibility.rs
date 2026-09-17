//! Opt-in accessibility smoke test for the Vox window itself.
//!
//! It needs a live Linux desktop session where the user's screen-reader
//! accessibility service is already enabled. The test deliberately does not
//! alter that system setting. Run it with:
//!
//! `VOX_TEST_A11Y=1 cargo test -p vox-desktop --test native_accessibility -- --ignored --nocapture`

#![cfg(target_os = "linux")]

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vox_desktop_access::{NativeDesktop, SemanticSnapshot};

struct VoxProcess {
    child: Child,
    data_dir: PathBuf,
}

impl VoxProcess {
    fn wait_for_exit(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for VoxProcess {
    fn drop(&mut self) {
        self.wait_for_exit(Duration::from_millis(250));
        let _ = fs::remove_dir_all(&self.data_dir);
    }
}

fn temporary_data_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("vox-native-a11y-{}-{nonce}", std::process::id()))
}

fn screen_reader_accessibility_is_enabled() -> bool {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.a11y.Bus",
            "--object-path",
            "/org/a11y/bus",
            "--method",
            "org.freedesktop.DBus.Properties.Get",
            "org.a11y.Status",
            "ScreenReaderEnabled",
        ])
        .output();
    output.ok().is_some_and(|output| {
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("true")
    })
}

fn start_vox() -> VoxProcess {
    let data_dir = temporary_data_dir();
    fs::create_dir_all(&data_dir).expect("temporary Vox data directory should be created");
    let child = Command::new(env!("CARGO_BIN_EXE_vox-desktop"))
        .env("VOX_DATA_DIR", &data_dir)
        .env("VOX_PROVIDER", "fake")
        .env("VOX_PROVIDER_API_KEY", "")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Vox window should start");
    VoxProcess { child, data_dir }
}

fn wait_for_snapshot(
    desktop: &NativeDesktop,
    app_pid: u32,
    required_label: &str,
) -> SemanticSnapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(snapshots) = desktop.snapshots_for_app_pid(Some(app_pid)) {
            if let Some(snapshot) = snapshots.into_iter().find(|snapshot| {
                snapshot
                    .nodes
                    .iter()
                    .any(|node| node.name == required_label)
            }) {
                return snapshot;
            }
        }
        assert!(
            Instant::now() < deadline,
            "Vox control {required_label:?} was not exposed to AT-SPI"
        );
        thread::sleep(Duration::from_millis(75));
    }
}

fn wait_for_pressable_snapshot(
    desktop: &NativeDesktop,
    app_pid: u32,
    required_label: &str,
) -> SemanticSnapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(snapshots) = desktop.snapshots_for_app_pid(Some(app_pid)) {
            if let Some(snapshot) = snapshots.into_iter().find(|snapshot| {
                snapshot.nodes.iter().any(|node| {
                    node.name == required_label
                        && node.actions.iter().any(|action| action == "press")
                })
            }) {
                return snapshot;
            }
        }
        assert!(
            Instant::now() < deadline,
            "Vox control {required_label:?} was not exposed as a pressable action"
        );
        thread::sleep(Duration::from_millis(75));
    }
}

fn press(snapshot: &SemanticSnapshot, desktop: &NativeDesktop, label: &str) {
    let element_ref = snapshot
        .nodes
        .iter()
        .find(|node| node.name == label && node.actions.iter().any(|action| action == "press"))
        .unwrap_or_else(|| panic!("Vox control {label:?} should be pressable"))
        .element_ref
        .clone();
    desktop
        .act(snapshot, &element_ref, "press", None)
        .unwrap_or_else(|error| {
            panic!("Vox control {label:?} should accept a semantic press: {error}")
        });
}

#[test]
#[ignore = "requires a live Linux AT-SPI session with screen-reader accessibility enabled"]
fn vox_window_exposes_its_controls_and_preferences_semantically() {
    assert_eq!(
        std::env::var("VOX_TEST_A11Y").as_deref(),
        Ok("1"),
        "set VOX_TEST_A11Y=1 to acknowledge this real-session test"
    );
    assert!(
        screen_reader_accessibility_is_enabled(),
        "enable your screen reader/accessibility service before running this test"
    );

    let vox = start_vox();
    let app_pid = vox.child.id();
    let desktop = NativeDesktop::default();
    let snapshot = wait_for_snapshot(&desktop, app_pid, "Mensagem");
    for label in ["Vox", "Mais", "Recolher", "Mensagem", "Enviar"] {
        assert!(
            snapshot.nodes.iter().any(|node| node.name == label),
            "Vox should expose the {label:?} label"
        );
    }
    assert!(snapshot.nodes.iter().any(|node| {
        node.role == "text_area"
            && node.name == "Mensagem"
            && node.actions.iter().any(|action| action == "press")
    }));

    let compact_button = wait_for_pressable_snapshot(&desktop, app_pid, "Recolher");
    press(&compact_button, &desktop, "Recolher");
    let compact = wait_for_snapshot(&desktop, app_pid, "Expandir");
    press(&compact, &desktop, "Expandir");
    let expanded = wait_for_snapshot(&desktop, app_pid, "Mais");

    press(&expanded, &desktop, "Mais");
    let menu = wait_for_snapshot(&desktop, app_pid, "Preferências");
    press(&menu, &desktop, "Preferências");
    let preferences = wait_for_snapshot(&desktop, app_pid, "Pastas permitidas");
    assert!(preferences
        .nodes
        .iter()
        .any(|node| node.name == "Acessibilidade: depende da permissão do sistema."));
    for label in ["Sistema", "Claro", "Escuro"] {
        assert!(
            preferences.nodes.iter().any(|node| node.name == label),
            "theme choice {label:?} should be exposed to assistive technology"
        );
    }
}

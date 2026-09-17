use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use thiserror::Error;
use uuid::Uuid;

/// A bounded, presentation-safe view of a native top-level window.
///
/// The xa11y element itself is deliberately not exposed outside this crate:
/// references and provider handles are valid only for the native query that
/// created them. Vox instead returns stable-looking metadata and requires a
/// fresh observation before an action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeWindow {
    pub app_name: String,
    pub app_pid: Option<u32>,
    pub window_id: String,
    pub name: String,
    pub role: String,
    pub states: Vec<String>,
    pub bounds: Option<[i32; 4]>,
}

/// Enumerate native top-level windows through the pinned xa11y adapter.
///
/// This function performs one bounded observation. It does not retain xa11y
/// handles, subscribe to events, or simulate success when the platform bridge
/// is unavailable. Callers can surface the returned error as a recoverable
/// capability/permission diagnostic.
pub fn list_native_windows() -> Result<Vec<NativeWindow>, String> {
    use xa11y::{App, AppExt};

    let apps = App::list().map_err(|error| format!("xa11y app enumeration failed: {error}"))?;
    let mut windows = Vec::new();
    for app in apps {
        let app_name = app.name.clone();
        let display_app_name = bounded_display_text(&app_name, MAX_DISPLAY_TEXT_CHARS);
        let app_pid = app.pid;
        let app_id = native_app_id(&app_name, app_pid, app.data.handle);
        let app_is_window = matches!(app.data.role, xa11y::Role::Window | xa11y::Role::Dialog);
        let app_windows = if app_is_window {
            Vec::new()
        } else {
            // Chromium/Electron applications can expose an application but
            // refuse a renderer accessibility tree until it is launched with
            // a separate accessibility flag. One such application must not
            // hide every other desktop window from the user or the broker.
            match app.children() {
                Ok(children) => children,
                Err(_) => continue,
            }
        };
        for window in app_windows
            .into_iter()
            .filter(|window| matches!(window.role, xa11y::Role::Window | xa11y::Role::Dialog))
        {
            let data = window.data();
            let window_id = native_window_ref(&app_id, data);
            let mut states = Vec::new();
            if data.states.enabled {
                states.push("enabled".into());
            }
            if data.states.visible {
                states.push("visible".into());
            }
            if data.states.active {
                states.push("active".into());
            }
            if data.states.focused {
                states.push("focused".into());
            }
            windows.push(NativeWindow {
                app_name: display_app_name.clone(),
                app_pid,
                window_id,
                name: bounded_display_text(
                    data.name.as_deref().unwrap_or_default(),
                    MAX_DISPLAY_TEXT_CHARS,
                ),
                role: data.role.to_snake_case().into(),
                states,
                bounds: data
                    .bounds
                    .map(|rect| [rect.x, rect.y, rect.width as i32, rect.height as i32]),
            });
        }
        if app_is_window {
            let data = &app.data;
            let window_id = native_window_ref(&app_id, data);
            windows.push(NativeWindow {
                app_name: display_app_name,
                app_pid,
                window_id,
                name: bounded_display_text(
                    data.name.as_deref().unwrap_or_default(),
                    MAX_DISPLAY_TEXT_CHARS,
                ),
                role: data.role.to_snake_case().into(),
                states: Vec::new(),
                bounds: data
                    .bounds
                    .map(|rect| [rect.x, rect.y, rect.width as i32, rect.height as i32]),
            });
        }
    }
    Ok(windows)
}

#[derive(Debug, Clone, Copy)]
pub struct NativeSnapshotLimits {
    pub max_nodes: usize,
    pub max_depth: usize,
}

impl Default for NativeSnapshotLimits {
    fn default() -> Self {
        Self {
            max_nodes: 512,
            max_depth: 8,
        }
    }
}

/// Native semantic observer used by the broker. It deliberately returns data
/// only: an action must re-resolve the app/window and verify the postcondition
/// before it can be reported as successful.
#[derive(Debug, Clone, Default)]
pub struct NativeDesktop {
    pub limits: NativeSnapshotLimits,
}

impl NativeDesktop {
    pub fn snapshots(&self) -> Result<Vec<SemanticSnapshot>, String> {
        self.snapshots_for_app_pid(None)
    }

    /// Observe either the full desktop or one process selected from the
    /// broker-owned window list. Targeting a process avoids walking unrelated
    /// accessibility trees after the agent has identified its intended app.
    pub fn snapshots_for_app_pid(
        &self,
        app_pid: Option<u32>,
    ) -> Result<Vec<SemanticSnapshot>, String> {
        use xa11y::{App, AppExt, Role};

        let apps = App::list().map_err(|error| format!("xa11y app enumeration failed: {error}"))?;
        let mut snapshots = Vec::new();
        let mut inaccessible_apps = Vec::new();
        for app in apps {
            if app_pid.is_some_and(|pid| app.pid != Some(pid)) {
                continue;
            }
            let app_id = native_app_id(&app.name, app.pid, app.data.handle);
            let windows = if matches!(app.data.role, Role::Window | Role::Dialog) {
                Vec::new()
            } else {
                match app.children() {
                    Ok(children) => children
                        .into_iter()
                        .filter(|element| matches!(element.role, Role::Window | Role::Dialog))
                        .collect(),
                    Err(_) => {
                        inaccessible_apps
                            .push(bounded_display_text(&app.name, MAX_DISPLAY_TEXT_CHARS));
                        continue;
                    }
                }
            };
            if windows.is_empty() && matches!(app.data.role, Role::Window | Role::Dialog) {
                snapshots.push(self.snapshot_for_data(&app_id, &app.data));
            } else {
                for window in windows {
                    match self.snapshot_for_element(&app_id, &window) {
                        Ok(snapshot) => snapshots.push(snapshot),
                        Err(_) => inaccessible_apps
                            .push(bounded_display_text(&app.name, MAX_DISPLAY_TEXT_CHARS)),
                    }
                }
            }
        }
        if snapshots.is_empty() && !inaccessible_apps.is_empty() {
            let inaccessible = inaccessible_apps
                .into_iter()
                .take(3)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "xa11y could not read an accessibility tree for: {inaccessible}"
            ));
        }
        Ok(snapshots)
    }

    pub fn wait_for(&self, query: &SemanticQuery, timeout: Duration) -> Result<QueryPage, String> {
        self.wait_for_with_cancel_for_app_pid(query, timeout, &AtomicBool::new(false), None)
    }

    pub fn wait_for_with_cancel(
        &self,
        query: &SemanticQuery,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<QueryPage, String> {
        self.wait_for_with_cancel_for_app_pid(query, timeout, cancel, None)
    }

    pub fn wait_for_with_cancel_for_app_pid(
        &self,
        query: &SemanticQuery,
        timeout: Duration,
        cancel: &AtomicBool,
        app_pid: Option<u32>,
    ) -> Result<QueryPage, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("accessibility wait cancelled".into());
            }
            let mut page = QueryPage {
                element_refs: Vec::new(),
                truncated: false,
            };
            for snapshot in self.snapshots_for_app_pid(app_pid)? {
                if cancel.load(Ordering::Relaxed) {
                    return Err("accessibility wait cancelled".into());
                }
                let current = query_snapshot(&snapshot, query);
                page.truncated |= current.truncated;
                page.element_refs.extend(current.element_refs);
                if !page.element_refs.is_empty() {
                    return Ok(page);
                }
            }
            if Instant::now() >= deadline {
                return Ok(page);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            std::thread::sleep(remaining.min(Duration::from_millis(50)));
        }
    }

    fn snapshot_for_element(
        &self,
        app_id: &str,
        element: &xa11y::Element,
    ) -> Result<SemanticSnapshot, String> {
        let data = element.data();
        let window_id = native_window_ref(app_id, data);
        let mut nodes = Vec::new();
        let mut truncated = false;
        append_native_node(element, None, 0, self.limits, &mut nodes, &mut truncated)?;
        Ok(self.finish_snapshot(app_id, window_id, nodes, truncated))
    }

    fn snapshot_for_data(&self, app_id: &str, data: &xa11y::ElementData) -> SemanticSnapshot {
        let mut truncated = false;
        let node = semantic_node(data, None, &mut truncated);
        self.finish_snapshot(
            app_id,
            native_window_ref(app_id, data),
            vec![node],
            truncated,
        )
    }

    fn finish_snapshot(
        &self,
        app_id: &str,
        window_id: String,
        nodes: Vec<SemanticNode>,
        truncated: bool,
    ) -> SemanticSnapshot {
        SemanticSnapshot {
            snapshot_id: Uuid::new_v4().to_string(),
            captured_at: format!("{}", unix_millis()),
            generation: 1,
            app_id: app_id.to_owned(),
            window_id,
            focused_element: nodes
                .iter()
                .find(|node| node.states.iter().any(|state| state == "focused"))
                .map(|node| node.element_ref.clone()),
            capabilities: vec![
                "query".into(),
                "focus".into(),
                "press".into(),
                "toggle".into(),
                "set-value".into(),
            ],
            nodes,
            truncated,
            continuation: truncated.then(|| "fresh-observation-required".into()),
        }
    }

    pub fn act(
        &self,
        snapshot: &SemanticSnapshot,
        element_ref: &str,
        action: &str,
        value: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        if snapshot.snapshot_id.is_empty() || snapshot.generation == 0 {
            return Err("snapshot reference is invalid".into());
        }
        let before = snapshot
            .nodes
            .iter()
            .find(|node| node.element_ref == element_ref)
            .ok_or_else(|| "element reference is stale or not present in snapshot".to_string())?;
        if !before.actions.iter().any(|available| available == action) {
            return Err(format!(
                "action {action} was not available in the supplied snapshot"
            ));
        }
        let sensitive = is_sensitive_semantic_node(before);
        if sensitive && matches!(action, "set_value" | "set-value") {
            return Err("editing a sensitive accessibility field is not supported".into());
        }
        let element = resolve_native_element(&snapshot.app_id, &snapshot.window_id, element_ref)?
            .ok_or_else(|| "element reference is stale or not found".to_string())?;
        // A node can change between the broker-owned snapshot and this native
        // resolution. Recheck the live label before writing so a recycled
        // stable reference cannot turn an ordinary input into a credential
        // field during that gap.
        if matches!(action, "set_value" | "set-value")
            && is_sensitive_accessibility_field(
                element.data().name.as_deref().unwrap_or_default(),
                &[],
            )
        {
            return Err("editing a sensitive accessibility field is not supported".into());
        }
        if !element
            .data()
            .actions
            .iter()
            .any(|available| available == action)
        {
            return Err(format!("action {action} is not available for this element"));
        }
        match action {
            "press" => element.press(),
            "focus" => element.focus(),
            "toggle" => element.toggle(),
            "set_value" | "set-value" => {
                element.set_value(value.ok_or_else(|| "action requires a value".to_string())?)
            }
            _ => return Err(format!("unsupported native action: {action}")),
        }
        .map_err(|error| format!("xa11y action failed: {error}"))?;

        let after = self
            .snapshots()
            .ok()
            .and_then(|snapshots| {
                snapshots.into_iter().find(|candidate| {
                    candidate.app_id == snapshot.app_id && candidate.window_id == snapshot.window_id
                })
            })
            .and_then(|fresh| {
                fresh
                    .nodes
                    .into_iter()
                    .find(|node| node.element_ref == element_ref)
            });
        let verified = match (action, after.as_ref()) {
            ("focus", Some(after)) => after.states.iter().any(|state| state == "focused"),
            ("toggle", Some(after)) => {
                after.value != before.value
                    || after.states.iter().any(|state| state == "checked")
                        != before.states.iter().any(|state| state == "checked")
            }
            ("set_value" | "set-value", Some(after)) if !sensitive => {
                value.is_some() && after.value.as_deref() == value
            }
            _ => false,
        };
        Ok(serde_json::json!({
            "element_ref": element_ref,
            "action": action,
            "verified": verified,
            "verification": {
                "observed": after.is_some(),
                "postcondition": if verified { "matched" } else { "unknown" },
                "sensitive": sensitive
            }
        }))
    }
}

fn resolve_native_element(
    app_id: &str,
    window_id: &str,
    element_ref: &str,
) -> Result<Option<xa11y::Element>, String> {
    use xa11y::{App, AppExt, Role};

    let apps = App::list().map_err(|error| format!("xa11y app enumeration failed: {error}"))?;
    for app in apps {
        let current_app_id = native_app_id(&app.name, app.pid, app.data.handle);
        if current_app_id != app_id {
            continue;
        }
        if matches!(app.data.role, Role::Window | Role::Dialog) {
            continue;
        }
        let roots = match app.children() {
            Ok(roots) => roots,
            // An inaccessible unrelated window should not turn a stale
            // reference into an arbitrary target. Treat it as unavailable and
            // keep looking only within the matching app identity.
            Err(_) => continue,
        };
        for root in roots {
            if native_window_ref(&current_app_id, root.data()) != window_id {
                continue;
            }
            if let Some(element) = find_native_element(&root, element_ref, 0, 8)? {
                return Ok(Some(element));
            }
        }
    }
    Ok(None)
}

fn find_native_element(
    element: &xa11y::Element,
    element_ref: &str,
    depth: usize,
    max_depth: usize,
) -> Result<Option<xa11y::Element>, String> {
    if native_element_ref(element.data()) == element_ref {
        return Ok(Some(element.clone()));
    }
    if depth >= max_depth {
        return Ok(None);
    }
    let children = match element.children() {
        Ok(children) => children,
        Err(_) => return Ok(None),
    };
    for child in children {
        if let Some(found) = find_native_element(&child, element_ref, depth + 1, max_depth)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn unix_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

const MAX_DISPLAY_TEXT_CHARS: usize = 128;

fn bounded_display_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let visible = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{visible}…")
    } else {
        visible
    }
}

fn opaque_identifier(namespace: &str, value: &str) -> String {
    // Never serialize a backend stable id directly: some accessibility
    // bridges put user-visible strings in it. A truncated SHA-256 digest is
    // sufficient for an in-process, broker-owned reference and is recomputed
    // from the current native element during revalidation.
    let digest = Sha256::digest(value.as_bytes());
    let suffix = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{namespace}:{suffix}")
}

fn native_app_id(name: &str, pid: Option<u32>, handle: u64) -> String {
    let name = opaque_identifier("xa11y-app", name);
    pid.map(|pid| format!("{name}:{pid}"))
        .unwrap_or_else(|| format!("{name}:handle-{handle}"))
}

fn native_element_ref(data: &xa11y::ElementData) -> String {
    native_element_ref_for_identity(data.stable_id.as_deref(), data.handle)
}

fn native_element_ref_for_identity(stable_id: Option<&str>, handle: u64) -> String {
    // xa11y exposes `stable_id` specifically for cross-snapshot correlation
    // (a D-Bus object path on Linux). Provider handles are transient, so they
    // are a fallback only when no stable platform identity is available.
    let identity = stable_id
        .map(|id| format!("stable\u{1f}{id}"))
        .unwrap_or_else(|| format!("handle\u{1f}{handle}"));
    opaque_identifier("xa11y-ref", &identity)
}

fn native_window_ref(app_id: &str, data: &xa11y::ElementData) -> String {
    // Some platform adapters use a generic stable id for each app's window
    // root. The app identity makes that root unique without feeding a
    // transient handle into later revalidation.
    let element_ref = native_element_ref(data);
    opaque_identifier("xa11y-window", &format!("{app_id}\u{1f}{element_ref}"))
}

fn semantic_node(
    data: &xa11y::ElementData,
    parent_ref: Option<String>,
    truncated: &mut bool,
) -> SemanticNode {
    let element_ref = native_element_ref(data);
    let raw_name = data.name.as_deref().unwrap_or_default();
    let name = bounded_display_text(raw_name, MAX_DISPLAY_TEXT_CHARS);
    if name != raw_name {
        *truncated = true;
    }
    let mut states = Vec::new();
    if data.states.enabled {
        states.push("enabled".into());
    }
    if data.states.visible {
        states.push("visible".into());
    }
    if data.states.focused {
        states.push("focused".into());
    }
    if data.states.active {
        states.push("active".into());
    }
    let sensitive = is_sensitive_accessibility_field(raw_name, &states);
    let value = if sensitive {
        data.value.as_ref().map(|_| "[REDACTED]".into())
    } else {
        data.value.as_deref().map(|value| {
            let bounded = bounded_display_text(value, MAX_DISPLAY_TEXT_CHARS);
            if bounded != value {
                *truncated = true;
            }
            bounded
        })
    };
    SemanticNode {
        element_ref,
        parent_ref,
        role: data.role.to_snake_case().into(),
        name,
        value,
        states,
        actions: supported_actions(&data.actions),
        bounds: data
            .bounds
            .map(|rect| [rect.x, rect.y, rect.width as i32, rect.height as i32]),
    }
}

fn supported_actions(actions: &[String]) -> Vec<String> {
    actions
        .iter()
        .filter(|action| {
            matches!(
                action.as_str(),
                "press" | "focus" | "toggle" | "set_value" | "set-value"
            )
        })
        .take(8)
        .cloned()
        .collect()
}

fn append_native_node(
    element: &xa11y::Element,
    parent_ref: Option<String>,
    depth: usize,
    limits: NativeSnapshotLimits,
    nodes: &mut Vec<SemanticNode>,
    truncated: &mut bool,
) -> Result<(), String> {
    if nodes.len() >= limits.max_nodes || depth > limits.max_depth {
        *truncated = true;
        return Ok(());
    }
    let data = element.data();
    let element_ref = native_element_ref(data);
    nodes.push(semantic_node(data, parent_ref, truncated));
    if depth == limits.max_depth {
        if element
            .children()
            .map(|children| !children.is_empty())
            .unwrap_or(false)
        {
            *truncated = true;
        }
        return Ok(());
    }
    let children = match element.children() {
        Ok(children) => children,
        Err(_) => {
            *truncated = true;
            return Ok(());
        }
    };
    for child in children {
        append_native_node(
            &child,
            Some(element_ref.clone()),
            depth + 1,
            limits,
            nodes,
            truncated,
        )?;
        if *truncated && nodes.len() >= limits.max_nodes {
            break;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticNode {
    pub element_ref: String,
    pub parent_ref: Option<String>,
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    pub states: Vec<String>,
    pub actions: Vec<String>,
    pub bounds: Option<[i32; 4]>,
}

/// Returns whether a semantic field can plausibly hold a credential. The
/// model never gets the value of such fields and may not set them; a person
/// can still use the operating system's normal input path. Labels are checked
/// as whole words so "tokenizer" does not accidentally become a secret field.
///
/// This deliberately covers the initial Portuguese UI as well as common
/// English labels. It is conservative because exposing or writing a secret is
/// worse than requiring manual entry for an ambiguously named text field.
pub fn is_sensitive_accessibility_field(name: &str, states: &[String]) -> bool {
    if states.iter().any(|state| {
        let state = state.trim();
        state.eq_ignore_ascii_case("password") || state.eq_ignore_ascii_case("sensitive")
    }) {
        return true;
    }

    name.to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .any(|word| {
            matches!(
                word,
                "password"
                    | "passwd"
                    | "passcode"
                    | "passphrase"
                    | "secret"
                    | "senha"
                    | "segredo"
                    | "credential"
                    | "credentials"
                    | "credencial"
                    | "credenciais"
                    | "token"
                    | "pin"
                    | "key"
                    | "chave"
            )
        })
}

/// Convenience form used by the native bridge and the broker so both enforce
/// exactly the same credential boundary.
pub fn is_sensitive_semantic_node(node: &SemanticNode) -> bool {
    is_sensitive_accessibility_field(&node.name, &node.states)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct SemanticQuery {
    pub role: Option<String>,
    pub name: Option<String>,
    pub name_contains: Option<String>,
    pub state: Option<String>,
    pub action: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryPage {
    pub element_refs: Vec<String>,
    pub truncated: bool,
}

pub fn query_snapshot(snapshot: &SemanticSnapshot, query: &SemanticQuery) -> QueryPage {
    let limit = query.limit.unwrap_or(32).clamp(1, 128);
    let name_contains = query.name_contains.as_deref().map(str::to_lowercase);
    let mut element_refs = Vec::new();
    let mut truncated = false;
    for node in &snapshot.nodes {
        let matches = query
            .role
            .as_deref()
            .map(|role| node.role == role)
            .unwrap_or(true)
            && query
                .name
                .as_deref()
                .map(|name| node.name == name)
                .unwrap_or(true)
            && name_contains
                .as_deref()
                .map(|name| node.name.to_lowercase().contains(name))
                .unwrap_or(true)
            && query
                .state
                .as_deref()
                .map(|state| node.states.iter().any(|candidate| candidate == state))
                .unwrap_or(true)
            && query
                .action
                .as_deref()
                .map(|action| node.actions.iter().any(|candidate| candidate == action))
                .unwrap_or(true);
        if matches {
            if element_refs.len() >= limit {
                truncated = true;
                break;
            }
            element_refs.push(node.element_ref.clone());
        }
    }
    QueryPage {
        element_refs,
        truncated: truncated || snapshot.truncated,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticSnapshot {
    pub snapshot_id: String,
    pub captured_at: String,
    pub generation: u64,
    pub app_id: String,
    pub window_id: String,
    pub focused_element: Option<String>,
    pub capabilities: Vec<String>,
    pub nodes: Vec<SemanticNode>,
    pub truncated: bool,
    pub continuation: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AccessError {
    #[error("element reference is stale")]
    StaleElement,
    #[error("selector is ambiguous")]
    Ambiguous,
    #[error("element not found")]
    NotFound,
    #[error("action is not available")]
    Unsupported,
    #[error("editing a sensitive accessibility field is not supported")]
    SensitiveField,
}

#[derive(Debug, Clone)]
pub struct MockDesktop {
    snapshot: SemanticSnapshot,
    values: BTreeMap<String, String>,
}

impl MockDesktop {
    pub fn fixture() -> Self {
        let nodes = vec![
            SemanticNode {
                element_ref: "node-window".into(),
                parent_ref: None,
                role: "window".into(),
                name: "Vox Fixture".into(),
                value: None,
                states: vec!["enabled".into(), "visible".into()],
                actions: vec!["activate".into()],
                bounds: Some([0, 0, 640, 480]),
            },
            SemanticNode {
                element_ref: "node-button-a".into(),
                parent_ref: Some("node-window".into()),
                role: "button".into(),
                name: "Safe action".into(),
                value: None,
                states: vec!["enabled".into()],
                actions: vec!["press".into(), "focus".into()],
                bounds: None,
            },
            SemanticNode {
                element_ref: "node-button-b".into(),
                parent_ref: Some("node-window".into()),
                role: "button".into(),
                name: "Safe action".into(),
                value: None,
                states: vec!["enabled".into()],
                actions: vec!["press".into(), "focus".into()],
                bounds: None,
            },
            SemanticNode {
                element_ref: "node-toggle".into(),
                parent_ref: Some("node-window".into()),
                role: "checkbox".into(),
                name: "Enable feature".into(),
                value: Some("false".into()),
                states: vec!["enabled".into(), "unchecked".into()],
                actions: vec!["toggle".into()],
                bounds: None,
            },
            SemanticNode {
                element_ref: "node-secret".into(),
                parent_ref: Some("node-window".into()),
                role: "textfield".into(),
                name: "Password".into(),
                value: Some("[REDACTED]".into()),
                states: vec!["enabled".into(), "password".into()],
                actions: vec!["focus".into(), "set-value".into()],
                bounds: None,
            },
            SemanticNode {
                element_ref: "node-dialog".into(),
                parent_ref: Some("node-window".into()),
                role: "dialog".into(),
                name: "Confirm action".into(),
                value: None,
                states: vec!["enabled".into(), "visible".into(), "modal".into()],
                actions: vec!["press".into(), "focus".into()],
                bounds: Some([120, 80, 360, 180]),
            },
            SemanticNode {
                element_ref: "node-list".into(),
                parent_ref: Some("node-window".into()),
                role: "list".into(),
                name: "Virtualized results".into(),
                value: None,
                states: vec!["enabled".into(), "visible".into()],
                actions: vec!["focus".into()],
                bounds: Some([20, 280, 600, 140]),
            },
            SemanticNode {
                element_ref: "node-list-item-visible".into(),
                parent_ref: Some("node-list".into()),
                role: "list_item".into(),
                name: "Visible result".into(),
                value: None,
                states: vec!["enabled".into(), "visible".into()],
                actions: vec!["press".into(), "focus".into()],
                bounds: None,
            },
            SemanticNode {
                element_ref: "node-list-item-recreated".into(),
                parent_ref: Some("node-list".into()),
                role: "list_item".into(),
                name: "Recreated result".into(),
                value: None,
                states: vec!["enabled".into()],
                actions: vec!["press".into(), "focus".into()],
                bounds: None,
            },
        ];
        Self {
            snapshot: SemanticSnapshot {
                snapshot_id: "fixture-snapshot-1".into(),
                captured_at: "synthetic".into(),
                generation: 1,
                app_id: "vox.fixture".into(),
                window_id: "fixture-window".into(),
                focused_element: Some("node-button-a".into()),
                capabilities: vec![
                    "query".into(),
                    "press".into(),
                    "toggle".into(),
                    "set-value".into(),
                ],
                nodes,
                truncated: false,
                continuation: None,
            },
            values: BTreeMap::from([("node-toggle".into(), "false".into())]),
        }
    }

    pub fn snapshot(&self) -> SemanticSnapshot {
        self.snapshot.clone()
    }

    pub fn query(&self, role: Option<&str>, name: Option<&str>) -> Result<String, AccessError> {
        let matches: Vec<&SemanticNode> = self
            .snapshot
            .nodes
            .iter()
            .filter(|node| {
                role.map(|value| node.role == value).unwrap_or(true)
                    && name.map(|value| node.name == value).unwrap_or(true)
            })
            .collect();
        match matches.as_slice() {
            [] => Err(AccessError::NotFound),
            [node] => Ok(node.element_ref.clone()),
            _ => Err(AccessError::Ambiguous),
        }
    }

    pub fn query_selector(&self, query: &SemanticQuery) -> QueryPage {
        query_snapshot(&self.snapshot, query)
    }

    pub fn act(
        &mut self,
        snapshot_id: &str,
        generation: u64,
        element_ref: &str,
        action: &str,
        value: Option<&str>,
    ) -> Result<serde_json::Value, AccessError> {
        if snapshot_id != self.snapshot.snapshot_id || generation != self.snapshot.generation {
            return Err(AccessError::StaleElement);
        }
        let node = self
            .snapshot
            .nodes
            .iter()
            .find(|node| node.element_ref == element_ref)
            .ok_or(AccessError::StaleElement)?;
        if !node.actions.iter().any(|available| available == action) {
            return Err(AccessError::Unsupported);
        }
        if is_sensitive_semantic_node(node) && matches!(action, "set_value" | "set-value") {
            return Err(AccessError::SensitiveField);
        }
        match action {
            "toggle" => {
                let current = self
                    .values
                    .get(element_ref)
                    .cloned()
                    .unwrap_or_else(|| "false".into());
                let next = if current == "true" { "false" } else { "true" };
                self.values.insert(element_ref.into(), next.into());
                if let Some(node) = self
                    .snapshot
                    .nodes
                    .iter_mut()
                    .find(|node| node.element_ref == element_ref)
                {
                    node.value = Some(next.into());
                    node.states
                        .retain(|state| state != "checked" && state != "unchecked");
                    node.states.push(
                        if next == "true" {
                            "checked"
                        } else {
                            "unchecked"
                        }
                        .into(),
                    );
                }
                self.snapshot.generation += 1;
                Ok(
                    serde_json::json!({"element_ref":element_ref,"action":action,"value":next,"verified":true}),
                )
            }
            "set_value" | "set-value" => {
                let next = value.ok_or(AccessError::Unsupported)?;
                self.values.insert(element_ref.into(), next.into());
                if let Some(node) = self
                    .snapshot
                    .nodes
                    .iter_mut()
                    .find(|node| node.element_ref == element_ref)
                {
                    node.value = Some(next.into());
                }
                self.snapshot.generation += 1;
                Ok(
                    serde_json::json!({"element_ref":element_ref,"action":action,"value":next,"verified":true}),
                )
            }
            _ => Ok(serde_json::json!({"element_ref":element_ref,"action":action,"verified":true})),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_wait_honors_cancellation_before_touching_accessibility() {
        let desktop = NativeDesktop::default();
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            desktop
                .wait_for_with_cancel(
                    &SemanticQuery::default(),
                    Duration::from_secs(1),
                    &cancelled,
                )
                .unwrap_err(),
            "accessibility wait cancelled"
        );
    }

    #[test]
    fn accessibility_text_and_identifiers_are_bounded_and_opaque() {
        let input = "x".repeat(MAX_DISPLAY_TEXT_CHARS + 1);
        let display = bounded_display_text(&input, MAX_DISPLAY_TEXT_CHARS);
        assert_eq!(display.chars().count(), MAX_DISPLAY_TEXT_CHARS + 1);
        assert!(display.ends_with('…'));

        let identity = opaque_identifier("test", &input);
        assert_eq!(identity, opaque_identifier("test", &input));
        assert!(!identity.contains(&input));
        assert_eq!(identity.len(), "test:".len() + 32);

        let first_element = native_element_ref_for_identity(Some("element-path"), 41);
        let repeated_element = native_element_ref_for_identity(Some("element-path"), 42);
        let different_element = native_element_ref_for_identity(Some("other-path"), 41);
        assert_eq!(first_element, repeated_element);
        assert_ne!(first_element, different_element);
        assert!(!first_element.contains("element-path"));
        assert!(first_element.starts_with("xa11y-ref:"));

        let first_window = opaque_identifier("xa11y-window", "app-a\u{1f}element-path");
        let second_window = opaque_identifier("xa11y-window", "app-b\u{1f}element-path");
        assert_ne!(first_window, second_window);
    }

    #[test]
    fn ambiguous_query_never_selects_first() {
        assert_eq!(
            MockDesktop::fixture().query(Some("button"), Some("Safe action")),
            Err(AccessError::Ambiguous)
        );
    }

    #[test]
    fn stale_reference_and_sensitive_field_blocking_are_explicit() {
        let mut desktop = MockDesktop::fixture();
        let snapshot = desktop.snapshot();
        assert_eq!(
            desktop.act(
                &snapshot.snapshot_id,
                snapshot.generation + 1,
                "node-toggle",
                "toggle",
                None
            ),
            Err(AccessError::StaleElement)
        );
        assert_eq!(
            desktop.act(
                &snapshot.snapshot_id,
                snapshot.generation,
                "node-secret",
                "set-value",
                Some("secret"),
            ),
            Err(AccessError::SensitiveField)
        );
    }

    #[test]
    fn sensitive_labels_cover_portuguese_credentials_without_false_substring_matches() {
        for label in [
            "Senha do provedor",
            "Token de acesso",
            "Credenciais",
            "Chave de API",
            "PIN",
        ] {
            assert!(
                is_sensitive_accessibility_field(label, &[]),
                "{label:?} should be treated as sensitive"
            );
        }
        assert!(is_sensitive_accessibility_field(
            "Campo genérico",
            &["password".into()]
        ));
        assert!(!is_sensitive_accessibility_field("Tokenizador", &[]));
        assert!(!is_sensitive_accessibility_field("Chaveamento", &[]));
    }

    #[test]
    fn adversarial_fixture_contains_modal_and_virtualized_nodes() {
        let desktop = MockDesktop::fixture();
        let snapshot = desktop.snapshot();
        assert!(snapshot.nodes.iter().any(|node| node.role == "dialog"));
        assert!(snapshot
            .nodes
            .iter()
            .any(|node| node.name == "Virtualized results"));
        assert!(snapshot
            .nodes
            .iter()
            .any(|node| node.name == "Recreated result"));
    }

    #[test]
    fn changing_a_fixture_node_invalidates_the_previous_generation() {
        let mut desktop = MockDesktop::fixture();
        let before = desktop.snapshot();
        desktop
            .act(
                &before.snapshot_id,
                before.generation,
                "node-toggle",
                "toggle",
                None,
            )
            .unwrap();
        assert_eq!(
            desktop.act(
                &before.snapshot_id,
                before.generation,
                "node-list-item-recreated",
                "press",
                None,
            ),
            Err(AccessError::StaleElement)
        );
    }

    #[test]
    fn selector_filters_state_action_and_caps_virtualized_results() {
        let desktop = MockDesktop::fixture();
        let page = desktop.query_selector(&SemanticQuery {
            role: Some("list_item".into()),
            state: Some("visible".into()),
            action: Some("press".into()),
            limit: Some(1),
            ..Default::default()
        });
        assert_eq!(page.element_refs, vec!["node-list-item-visible"]);
        assert!(!page.truncated);
    }
}

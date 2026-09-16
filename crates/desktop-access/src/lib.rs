use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
        let app_pid = app.pid;
        let app_is_window = matches!(app.data.role, xa11y::Role::Window | xa11y::Role::Dialog);
        let app_windows = if app_is_window {
            Vec::new()
        } else {
            app.children().map_err(|error| {
                format!("xa11y window enumeration failed for {app_name}: {error}")
            })?
        };
        for window in app_windows
            .into_iter()
            .filter(|window| matches!(window.role, xa11y::Role::Window | xa11y::Role::Dialog))
        {
            let data = window.data();
            let window_id = data
                .stable_id
                .clone()
                .unwrap_or_else(|| format!("xa11y-handle:{}", data.handle));
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
                app_name: app_name.clone(),
                app_pid,
                window_id,
                name: data.name.clone().unwrap_or_default(),
                role: data.role.to_snake_case().into(),
                states,
                bounds: data
                    .bounds
                    .map(|rect| [rect.x, rect.y, rect.width as i32, rect.height as i32]),
            });
        }
        if app_is_window {
            let data = &app.data;
            let window_id = data
                .stable_id
                .clone()
                .unwrap_or_else(|| format!("xa11y-handle:{}", data.handle));
            windows.push(NativeWindow {
                app_name,
                app_pid,
                window_id,
                name: data.name.clone().unwrap_or_default(),
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
        use xa11y::{App, AppExt, Role};

        let apps = App::list().map_err(|error| format!("xa11y app enumeration failed: {error}"))?;
        let mut snapshots = Vec::new();
        for app in apps {
            let app_id = app
                .pid
                .map(|pid| format!("{}:{pid}", app.name))
                .unwrap_or_else(|| app.name.clone());
            let windows = if matches!(app.data.role, Role::Window | Role::Dialog) {
                Vec::new()
            } else {
                app.children()
                    .map_err(|error| format!("xa11y children failed for {}: {error}", app.name))?
                    .into_iter()
                    .filter(|element| matches!(element.role, Role::Window | Role::Dialog))
                    .collect()
            };
            if windows.is_empty() && matches!(app.data.role, Role::Window | Role::Dialog) {
                snapshots.push(self.snapshot_for_data(&app_id, &app.data));
            } else {
                for window in windows {
                    snapshots.push(self.snapshot_for_element(&app_id, &window)?);
                }
            }
        }
        Ok(snapshots)
    }

    pub fn wait_for(&self, query: &SemanticQuery, timeout: Duration) -> Result<QueryPage, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut page = QueryPage {
                element_refs: Vec::new(),
                truncated: false,
            };
            for snapshot in self.snapshots()? {
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
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn snapshot_for_element(
        &self,
        app_id: &str,
        element: &xa11y::Element,
    ) -> Result<SemanticSnapshot, String> {
        let data = element.data();
        let window_id = native_element_ref(data);
        let mut nodes = Vec::new();
        let mut truncated = false;
        append_native_node(element, None, 0, self.limits, &mut nodes, &mut truncated)?;
        Ok(self.finish_snapshot(app_id, window_id, nodes, truncated))
    }

    fn snapshot_for_data(&self, app_id: &str, data: &xa11y::ElementData) -> SemanticSnapshot {
        self.finish_snapshot(
            app_id,
            native_element_ref(data),
            vec![semantic_node(data, None)],
            false,
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
        let element = resolve_native_element(&snapshot.app_id, &snapshot.window_id, element_ref)?
            .ok_or_else(|| "element reference is stale or not found".to_string())?;
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
                snapshots
                    .into_iter()
                    .find(|candidate| candidate.window_id == snapshot.window_id)
            })
            .and_then(|fresh| {
                fresh
                    .nodes
                    .into_iter()
                    .find(|node| node.element_ref == element_ref)
            });
        let sensitive = before.states.iter().any(|state| state == "password")
            || before.name.to_lowercase().contains("secret")
            || before.name.to_lowercase().contains("password");
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
        let current_app_id = app
            .pid
            .map(|pid| format!("{}:{pid}", app.name))
            .unwrap_or_else(|| app.name.clone());
        if current_app_id != app_id {
            continue;
        }
        if matches!(app.data.role, Role::Window | Role::Dialog) {
            continue;
        }
        let roots = app
            .children()
            .map_err(|error| format!("xa11y children failed for {}: {error}", app.name))?;
        for root in roots {
            if native_element_ref(root.data()) != window_id {
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
    let children = element
        .children()
        .map_err(|error| format!("xa11y tree traversal failed: {error}"))?;
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

fn native_element_ref(data: &xa11y::ElementData) -> String {
    data.stable_id
        .clone()
        .unwrap_or_else(|| format!("xa11y-handle:{}", data.handle))
}

fn semantic_node(data: &xa11y::ElementData, parent_ref: Option<String>) -> SemanticNode {
    let element_ref = native_element_ref(data);
    let lower_name = data.name.as_deref().unwrap_or_default().to_lowercase();
    let sensitive = lower_name.contains("password") || lower_name.contains("secret");
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
    SemanticNode {
        element_ref,
        parent_ref,
        role: data.role.to_snake_case().into(),
        name: data.name.clone().unwrap_or_default(),
        value: if sensitive {
            data.value.as_ref().map(|_| "[REDACTED]".into())
        } else {
            data.value.clone()
        },
        states,
        actions: data.actions.clone(),
        bounds: data
            .bounds
            .map(|rect| [rect.x, rect.y, rect.width as i32, rect.height as i32]),
    }
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
    nodes.push(semantic_node(data, parent_ref));
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
    let children = element
        .children()
        .map_err(|error| format!("xa11y tree traversal failed: {error}"))?;
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
            "set-value" => {
                let next = value.ok_or(AccessError::Unsupported)?;
                if node.states.iter().any(|state| state == "password") {
                    return Ok(
                        serde_json::json!({"element_ref":element_ref,"action":action,"value":"[REDACTED]","verified":true}),
                    );
                }
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
    fn ambiguous_query_never_selects_first() {
        assert_eq!(
            MockDesktop::fixture().query(Some("button"), Some("Safe action")),
            Err(AccessError::Ambiguous)
        );
    }

    #[test]
    fn stale_reference_and_secret_redaction_are_explicit() {
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
        let secret = desktop
            .act(
                &snapshot.snapshot_id,
                snapshot.generation,
                "node-secret",
                "set-value",
                Some("secret"),
            )
            .unwrap();
        assert_eq!(secret["value"], "[REDACTED]");
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

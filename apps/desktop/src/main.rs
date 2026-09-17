mod ui_state;
mod voice_capability;

use eframe::egui;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use ui_state::{ModelActivityMessage, ModelActivityRound, PresentationState, WindowMode};
use uuid::Uuid;
use voice_capability::VoiceCapability;
use vox_broker::{Broker, BrokerCall};
use vox_secrets::SecretStore;
use vox_session_store::{ApprovalRecord, EffectRecord, SessionStore};
use vox_supervisor::{default_entry, default_node, AgentSupervisor};
use vox_tool_runtime::RuntimeConfig;

fn provider_is_configured(base_url: &str, model: &str) -> bool {
    !base_url.trim().is_empty() && !model.trim().is_empty()
}

/// Endpoint, model and keyring-account fields are configuration metadata, not
/// places to keep a credential. Reject a value that looks like one instead of
/// redacting it and then saving a broken provider configuration.
fn provider_configuration_contains_secret(value: &str) -> bool {
    redact_activity_text(value) != value
}

fn provider_configuration_is_safe(base_url: &str, model: &str) -> bool {
    !provider_configuration_contains_secret(base_url)
        && !provider_configuration_contains_secret(model)
}

fn provider_keyring_environment(
    configured_account: &str,
    provider_is_configured: bool,
) -> Vec<(String, String)> {
    if !provider_is_configured || provider_configuration_contains_secret(configured_account) {
        return Vec::new();
    }
    if std::env::var("VOX_PROVIDER_API_KEY").is_ok_and(|key| !key.trim().is_empty()) {
        return Vec::new();
    }
    let account = if configured_account.trim().is_empty() {
        std::env::var_os("VOX_PROVIDER_ACCOUNT")
    } else {
        Some(configured_account.into())
    };
    let Some(account) = account else {
        return Vec::new();
    };
    let account = account.to_string_lossy();
    match SecretStore::native("vox").get(&account) {
        Ok(Some(secret)) => vec![("VOX_PROVIDER_API_KEY".into(), secret)],
        Ok(None) | Err(_) => Vec::new(),
    }
}

fn provider_core_environment(base_url: &str, model: &str) -> Vec<(String, String)> {
    if !provider_is_configured(base_url, model) || !provider_configuration_is_safe(base_url, model)
    {
        // Explicitly override an inherited provider credential/configuration
        // so a plain desktop launch stays in the non-effectful demo mode.
        return vec![
            ("VOX_PROVIDER".into(), "fake".into()),
            ("VOX_PROVIDER_API_KEY".into(), String::new()),
        ];
    }
    vec![
        ("VOX_PROVIDER".into(), "openai-compatible".into()),
        ("VOX_PROVIDER_BASE_URL".into(), base_url.to_owned()),
        ("VOX_PROVIDER_MODEL".into(), model.to_owned()),
    ]
}

const ALLOWED_ROOTS_PREFERENCE: &str = "allowed_roots";
const THEME_PREFERENCE: &str = "theme_preference";
const LEGACY_DARK_THEME_PREFERENCE: &str = "dark_theme";
const LAST_ACTIVE_SESSION_PREFERENCE: &str = "last_active_session_id";
const DRAFT_PERSIST_DEBOUNCE: Duration = Duration::from_millis(600);
// A turn can legitimately spend more than a few seconds waiting for a remote
// provider.  Core liveness is therefore based on the heartbeat/activity
// channel, rather than on the age of the original turn.start request.
const CORE_UNRESPONSIVE_TIMEOUT: Duration = Duration::from_secs(12);
// A real visual smoke found that 176 points clipped the composer. This keeps
// the invocation surface small while leaving space for its label, draft and
// primary action at the normal minimum size.
const INVOCATION_VIEWPORT_SIZE: [f32; 2] = [440.0, 280.0];
const CONVERSATION_VIEWPORT_SIZE: [f32; 2] = [440.0, 520.0];
const COMPACT_VIEWPORT_SIZE: [f32; 2] = [360.0, 88.0];
const NORMAL_MIN_INNER_SIZE: [f32; 2] = [320.0, 260.0];
const COMPACT_MIN_INNER_SIZE: [f32; 2] = [320.0, 88.0];

/// The saved choice is deliberately separate from egui's resolved theme: in
/// System mode, egui can follow an operating-system change while Vox retains
/// the user's intent across launches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    fn from_preferences(value: Option<&str>, legacy_dark_theme: Option<&str>) -> Self {
        match value {
            Some("system") => Self::System,
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => match legacy_dark_theme {
                Some("true") => Self::Dark,
                Some("false") => Self::Light,
                _ => Self::System,
            },
        }
    }

    const fn storage_value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    const fn as_egui(self) -> egui::ThemePreference {
        match self {
            Self::System => egui::ThemePreference::System,
            Self::Light => egui::ThemePreference::Light,
            Self::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// Semantic colors for the small native window. Keeping the palette here
/// makes the visible states (primary action, warning, error and focus) agree
/// with the interface specification instead of accumulating ad-hoc grays.
#[derive(Clone, Copy)]
struct VoxPalette {
    surface_base: egui::Color32,
    surface_raised: egui::Color32,
    surface_subtle: egui::Color32,
    text_primary: egui::Color32,
    text_secondary: egui::Color32,
    border_subtle: egui::Color32,
    accent_fill: egui::Color32,
    accent_text: egui::Color32,
    focus_ring: egui::Color32,
    success: egui::Color32,
    warning: egui::Color32,
    error: egui::Color32,
}

fn vox_palette(dark_theme: bool) -> VoxPalette {
    if dark_theme {
        VoxPalette {
            surface_base: egui::Color32::from_rgb(24, 26, 29),
            surface_raised: egui::Color32::from_rgb(34, 37, 42),
            surface_subtle: egui::Color32::from_rgb(43, 47, 53),
            text_primary: egui::Color32::from_rgb(243, 244, 246),
            text_secondary: egui::Color32::from_rgb(185, 192, 202),
            border_subtle: egui::Color32::from_rgb(69, 76, 86),
            accent_fill: egui::Color32::from_rgb(154, 187, 255),
            accent_text: egui::Color32::from_rgb(20, 35, 64),
            focus_ring: egui::Color32::from_rgb(180, 205, 255),
            success: egui::Color32::from_rgb(135, 215, 164),
            warning: egui::Color32::from_rgb(240, 197, 117),
            error: egui::Color32::from_rgb(255, 161, 170),
        }
    } else {
        VoxPalette {
            surface_base: egui::Color32::from_rgb(250, 250, 249),
            surface_raised: egui::Color32::from_rgb(255, 255, 255),
            surface_subtle: egui::Color32::from_rgb(240, 241, 242),
            text_primary: egui::Color32::from_rgb(32, 36, 43),
            text_secondary: egui::Color32::from_rgb(82, 91, 103),
            border_subtle: egui::Color32::from_rgb(212, 216, 222),
            accent_fill: egui::Color32::from_rgb(36, 89, 196),
            accent_text: egui::Color32::WHITE,
            focus_ring: egui::Color32::from_rgb(23, 77, 168),
            success: egui::Color32::from_rgb(33, 104, 61),
            warning: egui::Color32::from_rgb(134, 84, 10),
            error: egui::Color32::from_rgb(180, 35, 53),
        }
    }
}

fn vox_style(dark_theme: bool) -> egui::Style {
    let palette = vox_palette(dark_theme);
    let mut style = egui::Style::default();
    let mut visuals = if dark_theme {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let control_radius = egui::CornerRadius::same(8);
    let subtle_stroke = egui::Stroke::new(1.0, palette.border_subtle);
    let primary_stroke = egui::Stroke::new(1.0, palette.text_primary);

    visuals.override_text_color = Some(palette.text_primary);
    visuals.weak_text_color = Some(palette.text_secondary);
    visuals.faint_bg_color = palette.surface_subtle;
    visuals.extreme_bg_color = palette.surface_raised;
    visuals.text_edit_bg_color = Some(palette.surface_raised);
    visuals.code_bg_color = palette.surface_subtle;
    visuals.hyperlink_color = palette.focus_ring;
    visuals.warn_fg_color = palette.warning;
    visuals.error_fg_color = palette.error;
    visuals.selection.bg_fill = palette.accent_fill;
    visuals.selection.stroke = egui::Stroke::new(1.0, palette.accent_text);
    visuals.window_corner_radius = egui::CornerRadius::same(16);
    visuals.window_fill = palette.surface_raised;
    visuals.window_stroke = subtle_stroke;
    visuals.menu_corner_radius = control_radius;
    visuals.panel_fill = palette.surface_base;
    visuals.text_cursor.stroke = egui::Stroke::new(2.0, palette.focus_ring);
    visuals.button_frame = false;
    visuals.disabled_alpha = 0.55;

    visuals.widgets.noninteractive.bg_fill = palette.surface_raised;
    visuals.widgets.noninteractive.weak_bg_fill = palette.surface_raised;
    visuals.widgets.noninteractive.bg_stroke = subtle_stroke;
    visuals.widgets.noninteractive.corner_radius = control_radius;
    visuals.widgets.noninteractive.fg_stroke = primary_stroke;

    visuals.widgets.inactive.bg_fill = palette.surface_raised;
    visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    visuals.widgets.inactive.bg_stroke = subtle_stroke;
    visuals.widgets.inactive.corner_radius = control_radius;
    visuals.widgets.inactive.fg_stroke = primary_stroke;
    visuals.widgets.inactive.expansion = 0.0;

    visuals.widgets.hovered.bg_fill = palette.surface_subtle;
    visuals.widgets.hovered.weak_bg_fill = palette.surface_subtle;
    visuals.widgets.hovered.bg_stroke = subtle_stroke;
    visuals.widgets.hovered.corner_radius = control_radius;
    visuals.widgets.hovered.fg_stroke = primary_stroke;
    visuals.widgets.hovered.expansion = 0.0;

    visuals.widgets.active.bg_fill = palette.surface_raised;
    visuals.widgets.active.weak_bg_fill = palette.surface_subtle;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, palette.focus_ring);
    visuals.widgets.active.corner_radius = control_radius;
    visuals.widgets.active.fg_stroke = primary_stroke;
    visuals.widgets.active.expansion = 0.0;

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.window_margin = egui::Margin::same(16);
    style.spacing.menu_margin = egui::Margin::same(8);
    style.spacing.button_padding = egui::vec2(10.0, 8.0);
    style.spacing.interact_size = egui::vec2(40.0, 36.0);
    style.spacing.icon_spacing = 6.0;
    style
}

/// Configure both style variants once. In System mode, egui then switches
/// between them when eframe receives a theme change from the operating system.
fn install_vox_theme(ctx: &egui::Context, preference: ThemePreference) {
    ctx.options_mut(|options| {
        // A native environment may not expose its scheme. A light fallback
        // keeps the default aligned with Vox's paper-like invocation surface.
        options.fallback_theme = egui::Theme::Light;
    });
    ctx.set_style_of(egui::Theme::Light, vox_style(false));
    ctx.set_style_of(egui::Theme::Dark, vox_style(true));
    ctx.set_theme(preference.as_egui());
}

fn primary_button(label: &'static str, palette: VoxPalette) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(label)
            .color(palette.accent_text)
            .strong(),
    )
    .fill(palette.accent_fill)
    .stroke(egui::Stroke::new(1.0, palette.accent_fill))
    .corner_radius(8)
    .min_size(egui::vec2(88.0, 36.0))
}

fn stop_button(palette: VoxPalette) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new("Parar").color(palette.error).strong())
        .stroke(egui::Stroke::new(1.0, palette.error))
        .corner_radius(8)
        .min_size(egui::vec2(72.0, 36.0))
}

/// Canonicalize a user-visible directory selection. The broker receives only
/// these roots, so a path outside this list remains out of scope even when a
/// model proposes it. Empty lines make the preferences field easy to edit.
fn parse_allowed_roots(input: &str) -> Result<Vec<PathBuf>, String> {
    let raw_roots = input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if raw_roots.is_empty() {
        return Err("informe ao menos uma pasta existente".into());
    }

    let mut roots = Vec::new();
    for root in raw_roots {
        let canonical = root.canonicalize().map_err(|_| {
            format!(
                "a pasta não existe ou não está acessível: {}",
                root.display()
            )
        })?;
        if !canonical.is_dir() {
            return Err(format!(
                "o caminho não é uma pasta: {}",
                canonical.display()
            ));
        }
        if !roots.iter().any(|known: &PathBuf| known == &canonical) {
            roots.push(canonical);
        }
    }
    Ok(roots)
}

fn roots_as_preference(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Default to the normal user-facing folders, never to the entire home
/// directory. A development checkout under the home directory remains useful,
/// while sensitive home files stay out of scope by default.
fn default_allowed_roots() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .and_then(|value| PathBuf::from(value).canonicalize().ok());
    if let Some(home) = home.as_ref() {
        candidates.extend(["Desktop", "Documents", "Downloads"].map(|name| home.join(name)));
    }
    if let Ok(current) = std::env::current_dir() {
        let current = current.canonicalize().unwrap_or(current);
        if home
            .as_ref()
            .is_some_and(|home| current != *home && current.starts_with(home))
        {
            candidates.push(current);
        }
    }

    let mut roots = Vec::new();
    for candidate in candidates {
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if canonical.is_dir() && !roots.iter().any(|known: &PathBuf| known == &canonical) {
            roots.push(canonical);
        }
    }
    roots
}

fn configured_allowed_roots(
    store: Option<&SessionStore>,
) -> (Vec<PathBuf>, String, Option<String>) {
    let saved = store
        .and_then(|store| store.preference(ALLOWED_ROOTS_PREFERENCE).ok().flatten())
        .filter(|value| !value.trim().is_empty());
    if let Some(saved) = saved {
        match parse_allowed_roots(&saved) {
            Ok(roots) => {
                let input = roots_as_preference(&roots);
                return (roots, input, None);
            }
            Err(error) => {
                let roots = default_allowed_roots();
                let input = roots_as_preference(&roots);
                return (
                    roots,
                    input,
                    Some(format!(
                        "as pastas permitidas salvas não puderam ser usadas ({error}); foram restauradas as pastas padrão"
                    )),
                );
            }
        }
    }

    let roots = default_allowed_roots();
    let input = roots_as_preference(&roots);
    (roots, input, None)
}

fn runtime_config_for_roots(allowed_roots: Vec<PathBuf>) -> RuntimeConfig {
    RuntimeConfig {
        allowed_roots,
        xa11y_command: std::env::var_os("VOX_XA11Y_BIN").map(PathBuf::from),
        ..RuntimeConfig::default()
    }
}

/// Defense in depth for content displayed by the transparency UI. The core is
/// responsible for redacting every provider request, but the desktop repeats
/// the small set of common credential patterns before rendering an IPC event.
fn redact_activity_text(value: &str) -> String {
    let mut redacted = value.to_owned();
    for marker in [
        "Bearer ",
        "sk-",
        "api_key=",
        "api-key=",
        "access_token=",
        "token=",
        "secret=",
        "password=",
        "senha=",
        "segredo=",
        "chave=",
        "credencial=",
        "api_key:",
        "api-key:",
        "access_token:",
        "token:",
        "secret:",
        "password:",
        "senha:",
        "segredo:",
        "chave:",
        "credencial:",
        "\"api_key\":",
        "\"api-key\":",
        "\"access_token\":",
        "\"token\":",
        "\"secret\":",
        "\"password\":",
        "\"senha\":",
        "\"segredo\":",
        "\"chave\":",
        "\"credencial\":",
        "\"authorization\":",
        "\"autorização\":",
    ] {
        let mut cursor = 0;
        while let Some(relative) = find_ascii_case_insensitive(&redacted[cursor..], marker) {
            let start = cursor + relative;
            let mut value_start = start + marker.len();
            while redacted
                .as_bytes()
                .get(value_start)
                .is_some_and(u8::is_ascii_whitespace)
            {
                value_start += 1;
            }
            let quote = match redacted.as_bytes().get(value_start).copied() {
                Some(byte @ (b'\'' | b'\"')) => Some(byte),
                _ => None,
            };
            if quote.is_some() {
                value_start += 1;
            }
            let end = redacted[value_start..]
                .find(|character: char| {
                    quote.map_or_else(
                        || {
                            character.is_whitespace()
                                || matches!(character, '\'' | '\"' | ',' | '}')
                        },
                        |quote| character == quote as char,
                    )
                })
                .map(|offset| value_start + offset)
                .unwrap_or(redacted.len());
            if value_start < end {
                redacted.replace_range(value_start..end, "[REDACTED]");
                cursor = value_start + "[REDACTED]".len();
            } else {
                cursor = value_start.saturating_add(1);
            }
        }
    }
    redacted
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let needle = needle.as_bytes();
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|candidate| candidate.eq_ignore_ascii_case(needle))
}

fn redacted_value_for_display(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact_activity_text(text)),
        Value::Array(values) => {
            Value::Array(values.iter().map(redacted_value_for_display).collect())
        }
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase();
                    let sensitive = [
                        "api_key",
                        "api-key",
                        "access_token",
                        "access-token",
                        "authorization",
                        "autorização",
                        "cookie",
                        "credential",
                        "credencial",
                        "credenciais",
                        "password",
                        "senha",
                        "private_key",
                        "private-key",
                        "secret",
                        "segredo",
                        "token",
                        "chave",
                    ]
                    .iter()
                    .any(|needle| normalized.contains(needle));
                    let value = if sensitive {
                        Value::String("[REDACTED]".into())
                    } else {
                        redacted_value_for_display(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        value => value.clone(),
    }
}

/// Approval controls state the irreversible effect in their accessible label
/// instead of asking the person to map a generic “Autorizar” to a detail box.
fn approval_decision_labels(call: &BrokerCall) -> (&'static str, &'static str) {
    match call.tool.as_str() {
        "files.write" if call.arguments.get("overwrite").and_then(Value::as_bool) == Some(true) => {
            ("Substituir arquivo", "Cancelar alteração")
        }
        "files.write" => ("Gravar arquivo", "Cancelar alteração"),
        "files.move" => ("Mover arquivo", "Cancelar movimentação"),
        "clipboard.write" => ("Alterar área de transferência", "Cancelar alteração"),
        "process.terminate" => ("Encerrar processo", "Cancelar encerramento"),
        "apps.launch" => ("Abrir aplicativo", "Cancelar abertura"),
        "paths.open" => ("Abrir item", "Cancelar abertura"),
        "shell.exec" => ("Executar comando", "Cancelar comando"),
        "desktop.act" => ("Executar ação", "Cancelar ação"),
        _ => ("Autorizar ação", "Cancelar ação"),
    }
}

/// A queued approval has priority over the composer. This also protects the
/// small edge where an Enter event arrives in the same frame as the approval.
fn draft_submission_allowed(view: &PresentationState, approval_pending: bool) -> bool {
    view.active_run.is_none() && view.mode != WindowMode::Approval && !approval_pending
}

/// A single broker worker keeps tool execution off eframe's UI thread.  The
/// broker itself remains shared so short, in-memory policy operations (issuing
/// or revoking an approval) can stay synchronous with the user's click.
///
/// Each queued execution carries its own cancellation flag.  The UI can set
/// that flag immediately without waiting for the worker to receive another
/// command; cancellable runtimes observe it while they are running.
struct BrokerWorker {
    commands: Sender<BrokerWorkerCommand>,
    results: Receiver<BrokerWorkerResult>,
}

enum BrokerWorkerCommand {
    Execute {
        job_id: String,
        call: BrokerCall,
        cancel: Arc<AtomicBool>,
    },
}

struct BrokerWorkerResult {
    job_id: String,
    call: BrokerCall,
    result: Result<vox_broker::BrokerResult, String>,
    cancel_requested: bool,
}

struct ActiveBrokerExecution {
    job_id: String,
    call: BrokerCall,
    cancel: Arc<AtomicBool>,
}

/// The structured observation returned to the core after a broker call. Keeping
/// these fields together makes the durable journal, the model context, and the
/// supervisor receive the same redacted outcome.
struct ToolResult<'a> {
    status: &'a str,
    side_effect: &'a str,
    data: &'a Value,
    verification: Option<&'a Value>,
    error_code: Option<&'a str>,
    error: Option<&'a str>,
}

impl BrokerWorker {
    fn spawn(broker: Arc<Broker>) -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        thread::Builder::new()
            .name("vox-broker-worker".into())
            .spawn(move || {
                while let Ok(command) = command_rx.recv() {
                    match command {
                        BrokerWorkerCommand::Execute {
                            job_id,
                            call,
                            cancel,
                        } => {
                            // Do not begin an effect if the user stopped the
                            // run while this job was waiting in the queue.
                            let result = if cancel.load(Ordering::Relaxed) {
                                Err("tool execution cancelled before it started".into())
                            } else {
                                broker
                                    .execute_with_cancel(&call, cancel.as_ref())
                                    .map_err(|error| error.to_string())
                            };
                            let _ = result_tx.send(BrokerWorkerResult {
                                job_id,
                                call,
                                result,
                                cancel_requested: cancel.load(Ordering::Relaxed),
                            });
                        }
                    }
                }
            })
            .expect("failed to start the broker worker");
        Self {
            commands: command_tx,
            results: result_rx,
        }
    }

    fn execute(&self, job_id: String, call: BrokerCall, cancel: Arc<AtomicBool>) -> Result<(), ()> {
        self.commands
            .send(BrokerWorkerCommand::Execute {
                job_id,
                call,
                cancel,
            })
            .map_err(|_| ())
    }

    fn try_result(&self) -> Option<BrokerWorkerResult> {
        match self.results.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }

    #[cfg(test)]
    fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<BrokerWorkerResult, mpsc::RecvTimeoutError> {
        self.results.recv_timeout(timeout)
    }
}

struct VoxApp {
    supervisor: Option<AgentSupervisor>,
    session_id: String,
    view: PresentationState,
    store: Option<SessionStore>,
    storage_warning: Option<String>,
    broker: Arc<Broker>,
    broker_worker: BrokerWorker,
    active_broker_execution: Option<ActiveBrokerExecution>,
    keep_on_top: bool,
    theme_preference: ThemePreference,
    retention_days: u32,
    retention_input: String,
    provider_is_configured: bool,
    show_history: bool,
    show_preferences: bool,
    history_query: String,
    history_offset: usize,
    window_level_applied: bool,
    last_viewport_mode: WindowMode,
    pending_approval: Option<BrokerCall>,
    pending_approval_summary: Option<String>,
    active_store_run: Option<String>,
    draft_persist_pending: bool,
    last_draft_change: Option<Instant>,
    last_heartbeat_sent: Instant,
    last_heartbeat_received: Instant,
    restart_attempts: u8,
    delete_confirmation: Option<String>,
    core_environment: Vec<(String, String)>,
    provider_account: String,
    provider_secret_input: String,
    provider_endpoint_input: String,
    provider_model_input: String,
    provider_keyring_status: Option<String>,
    file_roots_input: String,
    file_roots_status: Option<String>,
    pending_runtime_config: Option<RuntimeConfig>,
    configuration_restart_pending: bool,
    desktop_lease_owner: Option<String>,
    last_lease_renewed: Instant,
    voice: VoiceCapability,
}

impl VoxApp {
    fn new() -> Self {
        let provisional_session_id = Uuid::new_v4().to_string();
        let (store, mut storage_warning) = persistent_store();
        let lease_candidate = Uuid::new_v4().to_string();
        let desktop_lease_owner = match store.as_ref() {
            Some(store) => match store
                .acquire_desktop_lease(&lease_candidate, Duration::from_secs(30))
            {
                Ok(true) => Some(lease_candidate),
                Ok(false) => {
                    storage_warning = Some("outra instância do Vox já controla o desktop".into());
                    None
                }
                Err(error) => {
                    storage_warning = Some(format!("lease do desktop indisponível: {error}"));
                    None
                }
            },
            None => None,
        };
        let can_start_core = store.is_none() || desktop_lease_owner.is_some();
        let keep_on_top = store
            .as_ref()
            .and_then(|store| store.preference("keep_on_top").ok().flatten())
            .map(|value| value == "true")
            .unwrap_or(false);
        let saved_theme_preference = store
            .as_ref()
            .and_then(|store| store.preference(THEME_PREFERENCE).ok().flatten());
        let legacy_dark_theme = store.as_ref().and_then(|store| {
            store
                .preference(LEGACY_DARK_THEME_PREFERENCE)
                .ok()
                .flatten()
        });
        let theme_preference = ThemePreference::from_preferences(
            saved_theme_preference.as_deref(),
            legacy_dark_theme.as_deref(),
        );
        let retention_days = store
            .as_ref()
            .and_then(|store| store.preference("retention_days").ok().flatten())
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        let mut provider_account = store
            .as_ref()
            .and_then(|store| store.preference("provider_account").ok().flatten())
            .or_else(|| std::env::var("VOX_PROVIDER_ACCOUNT").ok())
            .unwrap_or_default();
        let mut provider_endpoint_input = store
            .as_ref()
            .and_then(|store| store.preference("provider_base_url").ok().flatten())
            .or_else(|| std::env::var("VOX_PROVIDER_BASE_URL").ok())
            .unwrap_or_default();
        let mut provider_model_input = store
            .as_ref()
            .and_then(|store| store.preference("provider_model").ok().flatten())
            .or_else(|| std::env::var("VOX_PROVIDER_MODEL").ok())
            .unwrap_or_default();
        if provider_configuration_contains_secret(&provider_account) {
            provider_account.clear();
            storage_warning =
                Some("A conta do provider continha uma credencial e não foi usada.".into());
        }
        if !provider_configuration_is_safe(&provider_endpoint_input, &provider_model_input) {
            provider_endpoint_input.clear();
            provider_model_input.clear();
            storage_warning =
                Some("A configuração do provider continha uma credencial e não foi usada.".into());
        }
        let provider_is_configured =
            provider_is_configured(&provider_endpoint_input, &provider_model_input)
                && provider_configuration_is_safe(&provider_endpoint_input, &provider_model_input);
        let model_label = if provider_is_configured {
            format!(
                "Provider configurado · {}",
                if provider_model_input.trim().is_empty() {
                    "modelo não informado".into()
                } else {
                    provider_model_input.clone()
                }
            )
        } else {
            "Demonstração local · ferramentas desativadas".into()
        };
        let restored_session_id = if can_start_core {
            store.as_ref().and_then(|store| {
                let session_id = store
                    .preference(LAST_ACTIVE_SESSION_PREFERENCE)
                    .ok()
                    .flatten()?;
                store
                    .session_exists(&session_id)
                    .ok()
                    .filter(|exists| *exists)?;
                Some(session_id)
            })
        } else {
            None
        };
        let session_id = restored_session_id.clone().unwrap_or_else(|| {
            if can_start_core {
                store
                    .as_ref()
                    .and_then(|store| store.create_session("Vox").ok())
                    .unwrap_or(provisional_session_id)
            } else {
                provisional_session_id
            }
        });
        let (allowed_roots, file_roots_input, file_roots_warning) =
            configured_allowed_roots(store.as_ref());
        if storage_warning.is_none() {
            storage_warning = file_roots_warning;
        }
        let runtime_config = runtime_config_for_roots(allowed_roots);
        let mut core_environment =
            provider_core_environment(&provider_endpoint_input, &provider_model_input);
        core_environment.extend(provider_keyring_environment(
            &provider_account,
            provider_is_configured,
        ));
        let broker = Arc::new(Broker::new(runtime_config));
        let broker_worker = BrokerWorker::spawn(Arc::clone(&broker));
        let mut app = Self {
            supervisor: None,
            session_id,
            view: PresentationState::default(),
            store,
            storage_warning,
            broker,
            broker_worker,
            active_broker_execution: None,
            keep_on_top,
            theme_preference,
            retention_days,
            retention_input: retention_days.to_string(),
            provider_is_configured,
            show_history: false,
            show_preferences: false,
            history_query: String::new(),
            history_offset: 0,
            window_level_applied: false,
            last_viewport_mode: WindowMode::Invocation,
            pending_approval: None,
            pending_approval_summary: None,
            active_store_run: None,
            draft_persist_pending: false,
            last_draft_change: None,
            last_heartbeat_sent: Instant::now(),
            last_heartbeat_received: Instant::now(),
            restart_attempts: 0,
            delete_confirmation: None,
            core_environment,
            provider_account,
            provider_secret_input: String::new(),
            provider_endpoint_input,
            provider_model_input,
            provider_keyring_status: None,
            file_roots_input,
            file_roots_status: None,
            pending_runtime_config: None,
            configuration_restart_pending: false,
            desktop_lease_owner,
            last_lease_renewed: Instant::now(),
            voice: VoiceCapability::detect(),
        };
        app.view.model_label = model_label;
        if restored_session_id.is_some() {
            if let Some(store) = app.store.as_ref() {
                match store.messages(&app.session_id) {
                    Ok(messages) => {
                        app.view.messages = messages
                            .into_iter()
                            .filter(|message| message.role != "tool")
                            .map(|message| ui_state::UiMessage {
                                role: if message.role == "assistant" {
                                    "Vox".into()
                                } else {
                                    "Você".into()
                                },
                                content: message.content,
                            })
                            .collect();
                    }
                    Err(error) => {
                        app.storage_warning = Some(format!(
                            "não foi possível restaurar o histórico da sessão: {error}"
                        ));
                    }
                }
                match store.draft(&app.session_id) {
                    Ok(Some(draft)) => app.view.draft = draft,
                    Ok(None) => {}
                    Err(error) => {
                        app.storage_warning = Some(format!(
                            "não foi possível restaurar o rascunho da sessão: {error}"
                        ));
                    }
                }
            }
        }
        app.save_active_session_preference();
        if can_start_core {
            match AgentSupervisor::spawn_with_env(
                default_node(),
                default_entry(),
                &app.core_environment,
            ) {
                Ok(supervisor) => {
                    let initialized = supervisor.initialize().is_ok();
                    let opened = supervisor
                        .open_session("open-session", &app.session_id)
                        .is_ok();
                    app.view.connected = false;
                    app.view.status = if initialized && opened {
                        "conectando ao core…"
                    } else {
                        "core iniciado, handshake pendente"
                    }
                    .into();
                    app.supervisor = Some(supervisor);
                }
                Err(error) => app.view.fail(format!("core indisponível: {error}")),
            }
        } else {
            app.view.connected = false;
            app.view.status = "outra instância controla o desktop".into();
        }
        app
    }

    fn sync_viewport(&mut self, ctx: &egui::Context) {
        if self.last_viewport_mode == self.view.mode {
            return;
        }
        let (size, minimum_size) = match self.view.mode {
            WindowMode::Invocation => (INVOCATION_VIEWPORT_SIZE, NORMAL_MIN_INNER_SIZE),
            WindowMode::Conversation | WindowMode::Approval => {
                (CONVERSATION_VIEWPORT_SIZE, NORMAL_MIN_INNER_SIZE)
            }
            WindowMode::Compact => (COMPACT_VIEWPORT_SIZE, COMPACT_MIN_INNER_SIZE),
        };
        // The compact accompaniment intentionally has a smaller minimum than
        // the conversation. Raising it again before expanding prevents the
        // normal composer from being manually resized below its controls.
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
            minimum_size[0],
            minimum_size[1],
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            size[0], size[1],
        )));
        self.last_viewport_mode = self.view.mode;
    }

    fn save_active_session_preference(&self) {
        let Some(store) = self.store.as_ref() else {
            return;
        };
        if store.session_exists(&self.session_id).unwrap_or(false) {
            let _ = store.set_preference(LAST_ACTIVE_SESSION_PREFERENCE, &self.session_id, 1);
        }
    }

    fn schedule_draft_persist(&mut self) {
        self.draft_persist_pending = true;
        self.last_draft_change = Some(Instant::now());
    }

    /// Flush a changed composer value after a small quiet period. Forced
    /// calls fence a session transition and application shutdown so a draft
    /// cannot be lost between repaint frames.
    fn flush_draft_persist(&mut self, force: bool) -> bool {
        if !self.draft_persist_pending {
            return true;
        }
        let due = force
            || self
                .last_draft_change
                .is_none_or(|changed| changed.elapsed() >= DRAFT_PERSIST_DEBOUNCE);
        if !due {
            return true;
        }
        let session_id = self.session_id.clone();
        let draft = self.view.draft.clone();
        let result = self
            .store
            .as_ref()
            .map(|store| store.save_draft(&session_id, &draft));
        match result {
            Some(Ok(())) | None => {
                self.draft_persist_pending = false;
                self.last_draft_change = None;
                true
            }
            Some(Err(error)) => {
                self.storage_warning = Some(format!(
                    "não foi possível salvar o rascunho desta conversa: {error}"
                ));
                false
            }
        }
    }

    fn clear_persisted_draft(&mut self) {
        self.draft_persist_pending = false;
        self.last_draft_change = None;
        let session_id = self.session_id.clone();
        let result = self
            .store
            .as_ref()
            .map(|store| store.save_draft(&session_id, ""));
        if let Some(Err(error)) = result {
            // Retry from the idle repaint rather than letting a stale durable
            // value reappear after this successfully accepted turn.
            self.draft_persist_pending = true;
            self.last_draft_change = Some(Instant::now());
            self.storage_warning = Some(format!(
                "não foi possível limpar o rascunho enviado: {error}"
            ));
        }
    }

    fn send(&mut self) {
        let content = self.view.draft.trim().to_owned();
        if content.is_empty()
            || !draft_submission_allowed(&self.view, self.pending_approval.is_some())
            || !self.view.connected
        {
            return;
        }
        if self.active_broker_execution.is_some() {
            self.view.status = "aguardando a interrupção da ação anterior".into();
            return;
        }
        let Some(supervisor) = self.supervisor.as_ref() else {
            return;
        };
        let run_id = Uuid::new_v4().to_string();
        self.view
            .submit(redact_activity_text(&content), run_id.clone());
        // The current user message is appended after the lookup and excluded
        // by sequence, so the provider receives prior conversation once plus
        // the current request once. SessionStore returns only persisted,
        // redacted roles suitable for the model boundary.
        let stored_sequence = self.store.as_ref().and_then(|store| {
            store
                .next_message_sequence(&self.session_id)
                .map_err(|error| {
                    self.storage_warning = Some(format!(
                        "não foi possível preparar o histórico local para esta tarefa: {error}"
                    ));
                })
                .ok()
        });
        let current_sequence = stored_sequence.unwrap_or(self.view.messages.len() as i64);
        let context_lookup = self
            .store
            .as_ref()
            .map(|store| store.model_context_before(&self.session_id, current_sequence, 100));
        let context = match context_lookup {
            Some(Ok(messages)) => messages
                .into_iter()
                .map(|message| {
                    serde_json::json!({
                        "role": message.role,
                        "content": message.content,
                    })
                })
                .collect(),
            Some(Err(error)) => {
                self.storage_warning = Some(format!(
                    "histórico local indisponível para esta tarefa: {error}"
                ));
                Vec::new()
            }
            None => Vec::new(),
        };
        if let Err(error) = supervisor.start_turn_with_context(
            &Uuid::new_v4().to_string(),
            &self.session_id,
            &run_id,
            &content,
            "text",
            &context,
        ) {
            self.view.withdraw_submission_for(&run_id);
            self.view
                .fail(redact_activity_text(&format!("falha ao enviar: {error}")));
            return;
        }
        if let Some(store) = self.store.as_ref() {
            match store.start_run(&self.session_id, Some(&self.view.model_label)) {
                Ok(run) => self.active_store_run = Some(run.id),
                Err(error) => {
                    self.storage_warning = Some(format!(
                        "não foi possível registrar esta tarefa no histórico local: {error}"
                    ));
                }
            }
            if stored_sequence.is_some() {
                if let Err(error) =
                    store.append_message(&self.session_id, "user", &content, current_sequence)
                {
                    self.storage_warning = Some(format!(
                        "não foi possível registrar a mensagem no histórico local: {error}"
                    ));
                }
            }
        }
        self.view.draft.clear();
        self.clear_persisted_draft();
    }

    fn open_session(&mut self, session_id: String) {
        if self.view.active_run.is_some() || self.session_id == session_id {
            return;
        }
        if !self.flush_draft_persist(true) {
            return;
        }
        let Some(store) = self.store.as_ref() else {
            self.view.fail("armazenamento indisponível");
            return;
        };
        let messages = match store.messages(&session_id) {
            Ok(messages) => messages,
            Err(error) => {
                self.storage_warning = Some(format!(
                    "não foi possível carregar o histórico da conversa: {error}"
                ));
                return;
            }
        };
        let draft = match store.draft(&session_id) {
            Ok(draft) => draft.unwrap_or_default(),
            Err(error) => {
                self.storage_warning = Some(format!(
                    "não foi possível carregar o rascunho da conversa: {error}"
                ));
                return;
            }
        };
        self.session_id = session_id.clone();
        self.view.messages = messages
            .into_iter()
            .filter(|message| message.role != "tool")
            .map(|message| ui_state::UiMessage {
                role: if message.role == "assistant" {
                    "Vox".into()
                } else {
                    "Você".into()
                },
                content: message.content,
            })
            .collect();
        self.view.draft = draft;
        self.draft_persist_pending = false;
        self.last_draft_change = None;
        self.view.error = None;
        self.view.activity = None;
        self.view.model_activity.clear();
        self.view.show_activity = false;
        self.view.status = "sessão carregada".into();
        self.save_active_session_preference();
        if let Some(supervisor) = self.supervisor.as_ref() {
            match supervisor.open_session(&Uuid::new_v4().to_string(), &session_id) {
                Ok(()) => {
                    self.view.connected = false;
                    self.view.status = "abrindo sessão…".into();
                }
                Err(error) => self
                    .view
                    .fail(format!("não foi possível abrir a sessão: {error}")),
            }
        }
    }

    fn new_conversation(&mut self) {
        if self.view.active_run.is_some() {
            return;
        }
        if !self.flush_draft_persist(true) {
            return;
        }
        let Some(store) = self.store.as_ref() else {
            self.view.messages.clear();
            self.view.draft.clear();
            self.draft_persist_pending = false;
            self.last_draft_change = None;
            return;
        };
        let Ok(session_id) = store.create_session("Nova conversa") else {
            self.view.fail("não foi possível criar uma nova conversa");
            return;
        };
        self.session_id = session_id.clone();
        self.view.messages.clear();
        self.view.draft.clear();
        self.draft_persist_pending = false;
        self.last_draft_change = None;
        self.view.error = None;
        self.view.activity = None;
        self.view.model_activity.clear();
        self.view.show_activity = false;
        self.view.status = "nova conversa".into();
        self.save_active_session_preference();
        if let Some(supervisor) = self.supervisor.as_ref() {
            match supervisor.open_session(&Uuid::new_v4().to_string(), &session_id) {
                Ok(()) => {
                    self.view.connected = false;
                    self.view.status = "abrindo sessão…".into();
                }
                Err(error) => self
                    .view
                    .fail(format!("não foi possível abrir a sessão: {error}")),
            }
        }
    }

    fn delete_session(&mut self, session_id: &str) {
        let session_id = session_id.to_owned();
        if session_id == self.session_id {
            if !self.flush_draft_persist(true) {
                return;
            }
            let Some(store) = self.store.as_ref() else {
                self.view.fail("armazenamento indisponível");
                self.delete_confirmation = None;
                return;
            };
            let replacement = match store.create_session("Nova conversa") {
                Ok(session_id) => session_id,
                Err(error) => {
                    self.view.fail(format!(
                        "não foi possível criar a conversa substituta: {error}"
                    ));
                    self.delete_confirmation = None;
                    return;
                }
            };
            let deletion = store.delete_session(&session_id);
            match deletion {
                Ok(()) => {
                    self.session_id = replacement.clone();
                    self.view.messages.clear();
                    self.view.draft.clear();
                    self.draft_persist_pending = false;
                    self.last_draft_change = None;
                    self.view.error = None;
                    self.view.activity = None;
                    self.view.model_activity.clear();
                    self.view.show_activity = false;
                    self.view.status = "conversa excluída; nova conversa pronta".into();
                    self.save_active_session_preference();
                    if let Some(supervisor) = self.supervisor.as_ref() {
                        match supervisor.open_session(&Uuid::new_v4().to_string(), &replacement) {
                            Ok(()) => {
                                self.view.connected = false;
                                self.view.status = "abrindo sessão…".into();
                            }
                            Err(error) => self.view.fail(format!(
                                "não foi possível abrir a sessão substituta: {error}"
                            )),
                        }
                    }
                }
                Err(error) => {
                    let _ = store.delete_session(&replacement);
                    self.view.fail(format!("não foi possível excluir: {error}"));
                }
            }
            self.delete_confirmation = None;
            return;
        }
        let result = self
            .store
            .as_ref()
            .map(|store| store.delete_session(&session_id));
        match result {
            Some(Ok(())) => self.view.status = "conversa excluída".into(),
            Some(Err(error)) => self.view.fail(format!("não foi possível excluir: {error}")),
            None => self.view.fail("armazenamento indisponível"),
        }
        self.delete_confirmation = None;
    }

    fn save_bool_preference(&self, key: &str, value: bool) {
        if let Some(store) = self.store.as_ref() {
            let _ = store.set_preference(key, if value { "true" } else { "false" }, 1);
        }
    }

    fn save_theme_preference(&self) {
        if let Some(store) = self.store.as_ref() {
            let _ =
                store.set_preference(THEME_PREFERENCE, self.theme_preference.storage_value(), 1);
        }
    }

    fn save_provider_credential(&mut self) {
        let account = self.provider_account.trim().to_owned();
        if account.is_empty() {
            self.provider_keyring_status = Some("Informe uma conta para o keyring.".into());
            return;
        }
        if provider_configuration_contains_secret(&account) {
            self.provider_account.clear();
            self.provider_keyring_status =
                Some("A conta não foi salva porque parece conter uma credencial.".into());
            return;
        }
        if let Some(store) = self.store.as_ref() {
            let _ = store.set_preference("provider_account", &account, 1);
        }
        self.provider_account = account.clone();
        if self.provider_secret_input.is_empty() {
            self.refresh_core_environment();
            let applied = self.restart_core_for_configuration();
            self.provider_keyring_status = Some(if applied {
                "Conta salva; nenhuma nova chave foi gravada. A configuração foi aplicada ao core ocioso.".into()
            } else {
                "Conta salva; a configuração será aplicada quando a tarefa atual terminar.".into()
            });
            return;
        }
        match SecretStore::native("vox").set(&account, &self.provider_secret_input) {
            Ok(()) => {
                self.provider_secret_input.clear();
                self.refresh_core_environment();
                let applied = self.restart_core_for_configuration();
                self.provider_keyring_status = Some(if applied {
                    "Chave salva no keyring; a configuração foi aplicada ao core ocioso.".into()
                } else {
                    "Chave salva no keyring; a configuração será aplicada quando a tarefa atual terminar.".into()
                });
            }
            Err(error) => {
                self.provider_keyring_status = Some(format!("Não foi possível salvar: {error}"));
            }
        }
    }

    fn save_provider_settings(&mut self) {
        let endpoint = self.provider_endpoint_input.trim().to_owned();
        let model = self.provider_model_input.trim().to_owned();
        if !provider_configuration_is_safe(&endpoint, &model) {
            if provider_configuration_contains_secret(&endpoint) {
                self.provider_endpoint_input.clear();
            }
            if provider_configuration_contains_secret(&model) {
                self.provider_model_input.clear();
            }
            self.provider_keyring_status =
                Some("A configuração não foi salva porque parece conter uma credencial.".into());
            return;
        }
        if let Some(store) = self.store.as_ref() {
            let _ = store.set_preference("provider_base_url", &endpoint, 1);
            let _ = store.set_preference("provider_model", &model, 1);
        }
        self.refresh_core_environment();
        let applied = self.restart_core_for_configuration();
        self.provider_keyring_status = Some(if applied {
            "Configuração salva e aplicada ao core ocioso; a janela permaneceu aberta.".into()
        } else {
            "Configuração salva; ela será aplicada quando a tarefa atual terminar.".into()
        });
    }

    fn refresh_core_environment(&mut self) {
        self.provider_is_configured =
            provider_is_configured(&self.provider_endpoint_input, &self.provider_model_input)
                && provider_configuration_is_safe(
                    &self.provider_endpoint_input,
                    &self.provider_model_input,
                );
        let mut environment =
            provider_core_environment(&self.provider_endpoint_input, &self.provider_model_input);
        environment.extend(provider_keyring_environment(
            &self.provider_account,
            self.provider_is_configured,
        ));
        self.core_environment = environment;
        self.view.model_label = if self.provider_is_configured {
            format!(
                "Provider configurado · {}",
                self.provider_model_input.trim()
            )
        } else {
            "Demonstração local · ferramentas desativadas".into()
        };
    }

    fn replace_broker_runtime(&mut self, runtime_config: RuntimeConfig) {
        // This method is called only when no run is active. Replacing the
        // worker therefore cannot move a tool from one file scope to another
        // halfway through an operation. Dropping the old sender lets an idle
        // worker exit after it has observed the closed channel.
        self.revoke_pending_approval("invalidated");
        let broker = Arc::new(Broker::new(runtime_config));
        self.broker_worker = BrokerWorker::spawn(Arc::clone(&broker));
        self.broker = broker;
    }

    fn save_allowed_roots(&mut self) {
        let roots = match parse_allowed_roots(&self.file_roots_input) {
            Ok(roots) => roots,
            Err(error) => {
                self.file_roots_status = Some(format!("As pastas não foram aplicadas: {error}"));
                return;
            }
        };
        let normalized = roots_as_preference(&roots);
        if let Some(store) = self.store.as_ref() {
            if let Err(error) = store.set_preference(ALLOWED_ROOTS_PREFERENCE, &normalized, 1) {
                self.file_roots_status =
                    Some(format!("Não foi possível salvar as pastas: {error}"));
                return;
            }
        }
        self.file_roots_input = normalized;
        let runtime_config = runtime_config_for_roots(roots);
        if self.view.active_run.is_some() || self.active_broker_execution.is_some() {
            self.pending_runtime_config = Some(runtime_config);
            self.file_roots_status = Some(
                "Pastas salvas; o novo escopo será aplicado quando a tarefa atual terminar.".into(),
            );
            return;
        }
        self.replace_broker_runtime(runtime_config);
        self.file_roots_status = Some("Pastas permitidas aplicadas a novas tarefas.".into());
    }

    fn finish_store_run(&mut self, state: &str) {
        if let (Some(store), Some(run_id)) = (self.store.as_ref(), self.active_store_run.take()) {
            let _ = store.finish_run(&run_id, state);
        }
    }

    fn record_approval_state(&self, call: &BrokerCall, state: &str) {
        if let (Some(store), Some(approval_id)) =
            (self.store.as_ref(), call.authorization_ref.as_deref())
        {
            let _ = store.set_approval_state(approval_id, state);
        }
    }

    fn effect_key(call: &BrokerCall) -> String {
        format!("{}:{}", call.run_id, call.call_id)
    }

    fn effect_envelope(result: &ToolResult<'_>) -> Value {
        serde_json::json!({
            "data": result.data,
            "error_code": result.error_code,
            "error": result.error,
            "verification": result.verification,
        })
    }

    fn persist_tool_context(&self, call: &BrokerCall, result: &ToolResult<'_>) {
        if self.view.active_run.as_deref() != Some(call.run_id.as_str()) {
            return;
        }
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let redacted_data = redacted_value_for_display(result.data);
        let data = if serde_json::to_string(&redacted_data)
            .is_ok_and(|serialized| serialized.len() > 12 * 1024)
        {
            Value::String(
                "[resultado da ferramenta omitido do histórico por exceder o limite]".into(),
            )
        } else {
            redacted_data
        };
        let record = serde_json::json!({
            "type": "tool_result",
            "tool": call.tool,
            "status": result.status,
            "side_effect": result.side_effect,
            "data": data,
            "error_code": result.error_code,
            "error": result.error.map(redact_activity_text),
            "verification": result.verification.map(redacted_value_for_display),
        });
        let Ok(sequence) = store.next_message_sequence(&self.session_id) else {
            return;
        };
        let content = serde_json::to_string(&record).unwrap_or_else(|_| {
            "{\"type\":\"tool_result\",\"status\":\"error\",\"data\":\"[indisponível]\"}".into()
        });
        let _ = store.append_message(&self.session_id, "tool", &content, sequence);
    }

    fn complete_effect(&self, call: &BrokerCall, result: &ToolResult<'_>) {
        if let Some(store) = self.store.as_ref() {
            let persisted = store.complete_effect(
                &Self::effect_key(call),
                result.status,
                result.side_effect,
                &Self::effect_envelope(result),
            );
            if matches!(persisted, Ok(true)) {
                self.persist_tool_context(call, result);
            }
        }
    }

    fn send_durable_effect(&self, call: &BrokerCall, effect: &EffectRecord) {
        let envelope = serde_json::from_str::<Value>(&effect.data)
            .unwrap_or_else(|_| serde_json::json!({"data": effect.data}));
        let data = envelope.get("data").unwrap_or(&envelope);
        let unknown = effect.status == "pending" || effect.status == "unknown";
        let status = if unknown {
            "unknown"
        } else {
            effect.status.as_str()
        };
        let side_effect = if unknown {
            "unknown"
        } else {
            effect.side_effect.as_str()
        };
        let error_code = if unknown {
            Some("EFFECT_UNKNOWN")
        } else {
            envelope.get("error_code").and_then(Value::as_str)
        };
        let error = if unknown {
            Some("o efeito já foi iniciado e não pode ser repetido automaticamente")
        } else {
            envelope.get("error").and_then(Value::as_str)
        };
        let verification = envelope.get("verification");
        if let Some(supervisor) = self.supervisor.as_ref() {
            let _ = supervisor.send_tool_result_with_verification(
                &call.run_id,
                &call.call_id,
                &call.tool,
                status,
                side_effect,
                data,
                verification,
                error_code,
                error,
            );
        }
    }

    fn start_broker_execution(&mut self, call: BrokerCall) -> Result<(), String> {
        if self.active_broker_execution.is_some() {
            return Err("a execução anterior ainda está sendo interrompida".into());
        }
        let job_id = Uuid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        self.broker_worker
            .execute(job_id.clone(), call.clone(), Arc::clone(&cancel))
            .map_err(|_| "o worker de ferramentas foi encerrado".to_string())?;
        self.active_broker_execution = Some(ActiveBrokerExecution {
            job_id,
            call,
            cancel,
        });
        Ok(())
    }

    fn cancel_broker_execution_for(&mut self, run_id: &str) {
        let approval_call = self.active_broker_execution.as_ref().and_then(|execution| {
            if execution.call.run_id == run_id {
                execution.cancel.store(true, Ordering::Relaxed);
                execution
                    .call
                    .authorization_ref
                    .as_ref()
                    .map(|_| execution.call.clone())
            } else {
                None
            }
        });
        if let Some(call) = approval_call {
            if let Some(approval_id) = call.authorization_ref.as_deref() {
                // If authorization won the race, execute_with_cancel will
                // receive the cancellation flag; otherwise this prevents a
                // queued approval from ever crossing the effect boundary.
                if self.broker.revoke_approval(approval_id).is_ok() {
                    self.record_approval_state(&call, "cancelled");
                }
            }
        }
    }

    fn cancel_active_broker_execution(&mut self) {
        let run_id = self
            .active_broker_execution
            .as_ref()
            .map(|execution| execution.call.run_id.clone());
        if let Some(run_id) = run_id {
            self.cancel_broker_execution_for(&run_id);
        }
    }

    fn send_broker_result(&self, call: &BrokerCall, result: &ToolResult<'_>) {
        if let Some(supervisor) = self.supervisor.as_ref() {
            let _ = supervisor.send_tool_result_with_verification(
                &call.run_id,
                &call.call_id,
                &call.tool,
                result.status,
                result.side_effect,
                result.data,
                result.verification,
                result.error_code,
                result.error,
            );
        }
    }

    fn finish_cancelled_broker_execution(
        &mut self,
        call: &BrokerCall,
        run_is_active: bool,
        approval_was_consumed: bool,
    ) {
        let data = Value::Null;
        let result = ToolResult {
            status: "cancelled",
            side_effect: "unknown",
            data: &data,
            verification: None,
            error_code: Some("CANCELLED_DURING_EFFECT"),
            error: Some("a execução foi interrompida depois de iniciar"),
        };
        self.complete_effect(call, &result);
        if approval_was_consumed {
            self.record_approval_state(call, "consumed");
        }
        if run_is_active {
            self.view.status = "ação interrompida; verificando cancelamento".into();
        }
    }

    fn handle_broker_result(&mut self, outcome: BrokerWorkerResult) {
        let matches_active = self
            .active_broker_execution
            .as_ref()
            .is_some_and(|execution| execution.job_id == outcome.job_id);
        if !matches_active {
            return;
        }
        let active = self
            .active_broker_execution
            .take()
            .expect("matching broker execution must exist");
        let call = outcome.call;
        let run_is_active = self.view.active_run.as_deref() == Some(call.run_id.as_str());

        match outcome.result {
            Ok(result) if result.error_code.as_deref() == Some("APPROVAL_REQUIRED") => {
                if outcome.cancel_requested || !run_is_active {
                    let data = Value::Null;
                    let result = ToolResult {
                        status: "cancelled",
                        side_effect: "none",
                        data: &data,
                        verification: None,
                        error_code: Some("CANCELLED_BEFORE_EFFECT"),
                        error: Some("ação cancelada antes da execução"),
                    };
                    self.complete_effect(&call, &result);
                    if run_is_active {
                        self.view.status = "ação cancelada antes da confirmação".into();
                    }
                    return;
                }
                match self.broker.request_approval(&call) {
                    Ok(approval) => {
                        let mut pending = call;
                        pending.authorization_ref = Some(approval.approval_id.clone());
                        if let Some(store) = self.store.as_ref() {
                            let _ = store.record_approval(&ApprovalRecord {
                                id: approval.approval_id,
                                run_id: pending.run_id.clone(),
                                call_id: pending.call_id.clone(),
                                args_hash: approval.args_hash,
                                scope: approval.scope,
                                expires_at: approval.expires_at,
                                state: "pending".into(),
                            });
                        }
                        let approval_summary =
                            approval.summary.as_deref().map(redact_activity_text);
                        self.pending_approval_summary = approval_summary.clone();
                        self.pending_approval = Some(pending);
                        self.view.mode = WindowMode::Approval;
                        self.view.activity = Some(
                            approval_summary
                                .unwrap_or_else(|| "Ação aguardando sua confirmação".into()),
                        );
                        self.view.status = "aguardando sua confirmação".into();
                    }
                    Err(error) => {
                        let message = error.to_string();
                        let data = Value::Null;
                        let result = ToolResult {
                            status: "error",
                            side_effect: "none",
                            data: &data,
                            verification: None,
                            error_code: Some("BROKER_ERROR"),
                            error: Some(&message),
                        };
                        self.complete_effect(&call, &result);
                        if run_is_active {
                            self.send_broker_result(&call, &result);
                            self.view.fail_for(
                                &call.run_id,
                                format!("não foi possível criar aprovação: {error}"),
                            );
                        }
                    }
                }
            }
            Ok(result) => {
                let approval_was_consumed = call.authorization_ref.is_some()
                    && result.error_code.as_deref() != Some("INVALID_APPROVAL");
                let tool_result = ToolResult {
                    status: &result.status,
                    side_effect: &result.side_effect,
                    data: &result.data,
                    verification: result.verification.as_ref(),
                    error_code: result.error_code.as_deref(),
                    error: result.error.as_deref(),
                };
                self.complete_effect(&call, &tool_result);
                if result.error_code.as_deref() == Some("INVALID_APPROVAL") {
                    self.record_approval_state(&call, "invalidated");
                } else if approval_was_consumed {
                    self.record_approval_state(&call, "consumed");
                }
                if outcome.cancel_requested {
                    if run_is_active {
                        self.view.status = if result.status == "success" {
                            "a ação terminou antes do cancelamento".into()
                        } else {
                            "ação interrompida".into()
                        };
                    }
                    return;
                }
                if run_is_active {
                    self.send_broker_result(&call, &tool_result);
                }
            }
            Err(error) => {
                if outcome.cancel_requested {
                    self.finish_cancelled_broker_execution(
                        &call,
                        run_is_active,
                        active.call.authorization_ref.is_some(),
                    );
                    return;
                }
                let data = Value::Null;
                let result = ToolResult {
                    status: "error",
                    side_effect: "none",
                    data: &data,
                    verification: None,
                    error_code: Some("BROKER_ERROR"),
                    error: Some(&error),
                };
                self.complete_effect(&call, &result);
                if call.authorization_ref.is_some() {
                    self.record_approval_state(&call, "consumed");
                }
                if run_is_active {
                    self.send_broker_result(&call, &result);
                }
            }
        }
    }

    fn revoke_pending_approval(&mut self, state: &str) {
        let Some(call) = self.pending_approval.take() else {
            self.pending_approval_summary = None;
            return;
        };
        self.pending_approval_summary = None;
        if let Some(approval_id) = call.authorization_ref.as_deref() {
            let _ = self.broker.revoke_approval(approval_id);
        }
        self.record_approval_state(&call, state);
        if state == "cancelled" {
            let data = Value::Null;
            let result = ToolResult {
                status: "cancelled",
                side_effect: "none",
                data: &data,
                verification: None,
                error_code: Some("CANCELLED_BEFORE_EFFECT"),
                error: Some("ação cancelada antes da execução"),
            };
            self.complete_effect(&call, &result);
        }
    }

    fn stop(&mut self) {
        let run_id = self.view.active_run.clone();
        if let Some(run_id) = run_id.as_deref() {
            self.cancel_broker_execution_for(run_id);
        } else {
            self.cancel_active_broker_execution();
        }
        self.revoke_pending_approval("cancelled");
        let Some(run_id) = run_id else {
            return;
        };
        if let Some(supervisor) = self.supervisor.as_ref() {
            let _ = supervisor.cancel(&Uuid::new_v4().to_string(), &run_id);
            self.view.status = "parando…".into();
        }
    }

    fn restart_core_after_failure(&mut self) {
        if let Some(run_id) = self.view.active_run.clone() {
            self.cancel_broker_execution_for(&run_id);
        }
        self.revoke_pending_approval("invalidated");
        let configuration_was_pending = self.configuration_restart_pending;
        self.configuration_restart_pending = false;
        let pending_unknown = self
            .store
            .as_ref()
            .and_then(|store| {
                store
                    .reconcile_pending_effects(self.active_store_run.as_deref())
                    .ok()
            })
            .unwrap_or(0);
        self.finish_store_run("interrupted");
        let interrupted_run = self.view.active_run.take().is_some();
        if interrupted_run {
            self.view.activity = None;
            self.view.error = Some(if pending_unknown > 0 {
                format!(
                    "run interrompido pela queda do core; {pending_unknown} efeito(s) ficaram incertos e não serão repetidos"
                )
            } else {
                "run interrompido pela queda do core".into()
            });
        }
        if self.restart_attempts >= 3 {
            self.view.connected = false;
            self.view
                .fail("core encerrado; limite de reinícios atingido");
            return;
        }
        self.restart_attempts += 1;
        self.supervisor.take();
        match AgentSupervisor::spawn_with_env(
            default_node(),
            default_entry(),
            &self.core_environment,
        ) {
            Ok(supervisor) => {
                let initialized = supervisor.initialize().is_ok();
                let opened = supervisor
                    .open_session(&Uuid::new_v4().to_string(), &self.session_id)
                    .is_ok();
                self.view.connected = false;
                self.view.status = if initialized && opened {
                    "core reiniciado; aguardando handshake".into()
                } else {
                    "core reiniciado, handshake pendente".into()
                };
                if configuration_was_pending && initialized && opened {
                    self.provider_keyring_status = Some(
                        "Configuração pendente aplicada durante a recuperação do core.".into(),
                    );
                }
                self.last_heartbeat_sent = Instant::now();
                self.last_heartbeat_received = Instant::now();
                self.supervisor = Some(supervisor);
            }
            Err(error) => {
                self.view.connected = false;
                self.view
                    .fail(format!("não foi possível reiniciar o core: {error}"));
            }
        }
    }

    fn restart_core_for_configuration(&mut self) -> bool {
        if self.view.active_run.is_some() {
            self.configuration_restart_pending = true;
            self.provider_keyring_status = Some(
                "A configuração foi salva; pare a tarefa atual para aplicá-la com segurança."
                    .into(),
            );
            return false;
        }
        self.configuration_restart_pending = false;
        self.revoke_pending_approval("invalidated");
        self.supervisor.take();
        self.view.connected = false;
        self.view.status = "aplicando configuração…".into();
        self.last_heartbeat_sent = Instant::now();
        self.last_heartbeat_received = Instant::now();
        match AgentSupervisor::spawn_with_env(
            default_node(),
            default_entry(),
            &self.core_environment,
        ) {
            Ok(supervisor) => {
                let initialized = supervisor.initialize().is_ok();
                let opened = supervisor
                    .open_session(&Uuid::new_v4().to_string(), &self.session_id)
                    .is_ok();
                let handshake_requested = initialized && opened;
                self.view.connected = false;
                self.view.status = if handshake_requested {
                    "configuração aplicada; aguardando handshake".into()
                } else {
                    "core reiniciado, handshake pendente".into()
                };
                self.supervisor = Some(supervisor);
                handshake_requested
            }
            Err(error) => {
                self.view
                    .fail(format!("não foi possível aplicar configuração: {error}"));
                false
            }
        }
    }

    fn apply_pending_configuration_if_idle(&mut self) {
        if self.view.active_run.is_none() && self.active_broker_execution.is_none() {
            if let Some(runtime_config) = self.pending_runtime_config.take() {
                self.replace_broker_runtime(runtime_config);
                self.file_roots_status =
                    Some("As pastas permitidas salvas foram aplicadas a novas tarefas.".into());
            }
        }
        if !self.configuration_restart_pending || self.view.active_run.is_some() {
            return;
        }
        self.configuration_restart_pending = false;
        let applied = self.restart_core_for_configuration();
        self.provider_keyring_status = Some(if applied {
            "Configuração pendente aplicada ao core ocioso; a janela permaneceu aberta.".into()
        } else {
            "A configuração pendente não pôde ser aplicada; tente salvar novamente.".into()
        });
    }

    fn approve_pending(&mut self) {
        let Some(call) = self.pending_approval.take() else {
            return;
        };
        self.pending_approval_summary = None;
        if self.view.active_run.as_deref() != Some(call.run_id.as_str()) {
            if let Some(approval_id) = call.authorization_ref.as_deref() {
                let _ = self.broker.revoke_approval(approval_id);
            }
            self.record_approval_state(&call, "cancelled");
            return;
        }
        match self.start_broker_execution(call.clone()) {
            Ok(()) => {
                self.view.mode = WindowMode::Conversation;
                self.view.status = "ação autorizada; executando".into();
            }
            Err(error) => {
                if let Some(approval_id) = call.authorization_ref.as_deref() {
                    let _ = self.broker.revoke_approval(approval_id);
                }
                self.record_approval_state(&call, "invalidated");
                let data = Value::Null;
                let result = ToolResult {
                    status: "error",
                    side_effect: "none",
                    data: &data,
                    verification: None,
                    error_code: Some("BROKER_WORKER_UNAVAILABLE"),
                    error: Some(&error),
                };
                self.complete_effect(&call, &result);
                self.send_broker_result(&call, &result);
                self.view
                    .fail(format!("não foi possível iniciar a ação: {error}"));
            }
        }
    }

    fn deny_pending(&mut self) {
        let Some(call) = self.pending_approval.take() else {
            return;
        };
        self.pending_approval_summary = None;
        if let Some(approval_id) = call.authorization_ref.as_deref() {
            let _ = self.broker.revoke_approval(approval_id);
        }
        self.record_approval_state(&call, "denied");
        let data = Value::Null;
        let result = ToolResult {
            status: "cancelled",
            side_effect: "none",
            data: &data,
            verification: None,
            error_code: Some("USER_DENIED"),
            error: Some("ação negada pelo usuário"),
        };
        self.complete_effect(&call, &result);
        self.send_broker_result(&call, &result);
        self.view.mode = WindowMode::Conversation;
        self.view.status = "ação negada".into();
    }

    fn handle_event(&mut self, event: Value) {
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        match kind {
            "initialized" => {
                self.view.connected = false;
                self.view.status = "abrindo sessão…".into();
                self.last_heartbeat_received = Instant::now();
            }
            "session.opened" => {
                if event.get("session_id").and_then(Value::as_str) == Some(self.session_id.as_str())
                {
                    self.view.connected = true;
                    self.view.status = "pronto".into();
                }
            }
            "heartbeat" => {
                self.last_heartbeat_received = Instant::now();
                if self.view.connected && self.view.active_run.is_none() {
                    self.view.status = "pronto".into();
                }
            }
            "state.changed" => {
                let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                    return;
                };
                if self.view.active_run.as_deref() != Some(run_id) {
                    return;
                }
                if let Some(state) = event.get("state").and_then(Value::as_str) {
                    self.view.status = match state {
                        "thinking" => "pensando".into(),
                        "executing" => "executando uma ação".into(),
                        "cancelling" => "parando…".into(),
                        other => other.into(),
                    };
                }
            }
            "model.requested" => {
                let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                    return;
                };
                if self.view.active_run.as_deref() != Some(run_id) {
                    return;
                }
                if event.get("redacted").and_then(Value::as_bool) != Some(true) {
                    self.view.fail_for(
                        run_id,
                        "o core tentou mostrar uma solicitação de modelo sem redação",
                    );
                    return;
                }
                let Some(round) = event.get("round").and_then(Value::as_u64) else {
                    self.view
                        .fail_for(run_id, "atividade do modelo recebida sem rodada válida");
                    return;
                };
                let Some(provider) = event.get("provider").and_then(Value::as_str) else {
                    self.view
                        .fail_for(run_id, "atividade do modelo recebida sem provedor válido");
                    return;
                };
                let Some(raw_messages) = event.get("messages").and_then(Value::as_array) else {
                    self.view
                        .fail_for(run_id, "atividade do modelo recebida sem mensagens válidas");
                    return;
                };
                let messages = raw_messages
                    .iter()
                    .filter_map(|message| {
                        let role = message.get("role").and_then(Value::as_str)?;
                        let content = message.get("content").and_then(Value::as_str)?;
                        Some(ModelActivityMessage {
                            role: role.into(),
                            content: redact_activity_text(content),
                        })
                    })
                    .collect::<Vec<_>>();
                if messages.is_empty() {
                    self.view
                        .fail_for(run_id, "atividade do modelo não contém mensagens exibíveis");
                    return;
                }
                self.view.record_model_activity_for(
                    run_id,
                    ModelActivityRound {
                        round,
                        provider: provider.into(),
                        model_ref: event
                            .get("model_ref")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        messages,
                    },
                );
            }
            "message.delta" => {
                let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                    return;
                };
                let seq = event.get("seq").and_then(Value::as_u64).unwrap_or(0);
                let delta = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.view
                    .append_delta_for(run_id, seq, &redact_activity_text(delta));
            }
            "tool.execute.requested" => {
                let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                    return;
                };
                if self.view.active_run.as_deref() != Some(run_id) {
                    return;
                }
                let requested_tool = event.get("tool").and_then(Value::as_str).unwrap_or("ação");
                self.view.activity = Some(format!("Executando {requested_tool}"));
                if let (Some(call_id), Some(tool)) = (
                    event.get("call_id").and_then(Value::as_str),
                    event.get("tool").and_then(Value::as_str),
                ) {
                    let arguments = event
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}));
                    let rendered_arguments = serde_json::to_string(&arguments).unwrap_or_default();
                    if redact_activity_text(&rendered_arguments) != rendered_arguments {
                        self.view.fail_for(
                            run_id,
                            "o core tentou usar uma credencial em uma ferramenta",
                        );
                        return;
                    }
                    let call = BrokerCall {
                        call_id: call_id.into(),
                        run_id: run_id.into(),
                        tool: tool.into(),
                        arguments,
                        authorization_ref: None,
                    };
                    if let Some(store) = self.store.as_ref() {
                        match store.effect(&Self::effect_key(&call)) {
                            Ok(Some(effect)) => {
                                self.send_durable_effect(&call, &effect);
                                self.view.status = if effect.status == "success" {
                                    "efeito já registrado; não repeti a ação".into()
                                } else {
                                    "efeito incerto recuperado; repetição bloqueada".into()
                                };
                                return;
                            }
                            Err(error) => {
                                self.view
                                    .fail(format!("journal de efeitos indisponível: {error}"));
                                return;
                            }
                            Ok(None) => {
                                if let Err(error) = store.begin_effect(
                                    &Self::effect_key(&call),
                                    &call.run_id,
                                    &call.call_id,
                                    &call.tool,
                                    &serde_json::json!({"arguments": call.arguments.clone()}),
                                ) {
                                    self.view.fail(format!(
                                        "não foi possível registrar a intenção: {error}"
                                    ));
                                    return;
                                }
                            }
                        }
                    }
                    if let Err(error) = self.start_broker_execution(call.clone()) {
                        let data = Value::Null;
                        let result = ToolResult {
                            status: "error",
                            side_effect: "none",
                            data: &data,
                            verification: None,
                            error_code: Some("BROKER_WORKER_UNAVAILABLE"),
                            error: Some(&error),
                        };
                        self.complete_effect(&call, &result);
                        self.send_broker_result(&call, &result);
                        self.view.fail_for(
                            run_id,
                            format!("não foi possível iniciar a ferramenta: {error}"),
                        );
                    }
                }
            }
            "tool.completed" => {
                let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                    return;
                };
                if self.view.active_run.as_deref() != Some(run_id) {
                    return;
                }
                let status = event
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("error");
                let observed = event
                    .get("verification")
                    .and_then(|value| value.get("observed"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.view.status = match (status, observed) {
                    ("success", true) => "resultado verificado".into(),
                    ("unknown", _) => "efeito incerto; confira antes de repetir".into(),
                    ("cancelled", _) => "ação cancelada".into(),
                    _ => "ação não concluída".into(),
                };
            }
            "approval.required" => {
                let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                    return;
                };
                if self.view.active_run.as_deref() != Some(run_id) {
                    return;
                }
                self.view.mode = WindowMode::Approval;
                self.view.activity = Some("Preciso da sua confirmação antes de continuar".into());
            }
            "run.completed" => {
                let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                    return;
                };
                if self.view.active_run.as_deref() != Some(run_id) {
                    return;
                }
                self.cancel_broker_execution_for(run_id);
                self.finish_store_run("completed");
                if let Some(content) = event.get("content").and_then(Value::as_str) {
                    let content = redact_activity_text(content);
                    self.view.complete_for(run_id, &content);
                    if let Some(store) = self.store.as_ref() {
                        if let Ok(sequence) = store.next_message_sequence(&self.session_id) {
                            let _ = store.append_message(
                                &self.session_id,
                                "assistant",
                                &content,
                                sequence,
                            );
                        }
                    }
                }
            }
            "run.cancelled" => {
                let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                    return;
                };
                if self.view.active_run.as_deref() != Some(run_id) {
                    return;
                }
                self.cancel_broker_execution_for(run_id);
                self.finish_store_run("cancelled");
                self.view.cancel_for(run_id);
            }
            "run.failed" => {
                let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                    return;
                };
                if self.view.active_run.as_deref() != Some(run_id) {
                    return;
                }
                self.cancel_broker_execution_for(run_id);
                self.finish_store_run("failed");
                let message = event
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("falha");
                self.view.fail_for(run_id, redact_activity_text(message));
            }
            "error" => {
                // An unscoped core diagnostic can describe a stale or
                // rejected IPC message. It must not terminate whichever
                // unrelated run happens to be active in the UI.
                let message = event
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("falha de protocolo do core");
                self.view.error = Some(format!("Core: {}", redact_activity_text(message)));
                if self.view.active_run.is_none() {
                    self.view.status = "core reportou um erro".into();
                }
            }
            _ => {}
        }
    }
}

fn persistent_store() -> (Option<SessionStore>, Option<String>) {
    let directory = std::env::var_os("VOX_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            #[cfg(target_os = "linux")]
            {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .or_else(|| {
                        std::env::var_os("HOME")
                            .map(|home| PathBuf::from(home).join(".local/share"))
                    })
                    .map(|base| base.join("vox"))
            }
            #[cfg(target_os = "macos")]
            {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join("Library/Application Support/Vox"))
            }
            #[cfg(target_os = "windows")]
            {
                std::env::var_os("APPDATA").map(|base| PathBuf::from(base).join("Vox"))
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
            {
                None
            }
        })
        .unwrap_or_else(|| PathBuf::from(".runtime-data/vox"));
    if fs::create_dir_all(&directory).is_err() {
        return (
            None,
            Some(format!(
                "não foi possível criar o diretório de dados: {}",
                directory.display()
            )),
        );
    }
    match SessionStore::open(directory.join("sessions.sqlite")) {
        Ok(store) => (Some(store), None),
        Err(error) => (None, Some(format!("histórico local indisponível: {error}"))),
    }
}

impl Drop for VoxApp {
    fn drop(&mut self) {
        let _ = self.flush_draft_persist(true);
        self.cancel_active_broker_execution();
        if let (Some(store), Some(owner)) =
            (self.store.as_ref(), self.desktop_lease_owner.as_deref())
        {
            let _ = store.release_desktop_lease(owner);
        }
    }
}

impl eframe::App for VoxApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.window_level_applied {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.keep_on_top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                }));
            self.window_level_applied = true;
        }
        if self.last_lease_renewed.elapsed() >= Duration::from_secs(5) {
            let lease_ok = match (self.store.as_ref(), self.desktop_lease_owner.as_deref()) {
                (Some(store), Some(owner)) => store.renew_desktop_lease(owner).unwrap_or(false),
                _ => true,
            };
            self.last_lease_renewed = Instant::now();
            if !lease_ok {
                if self.view.active_run.is_some() {
                    self.stop();
                }
                self.supervisor.take();
                self.desktop_lease_owner = None;
                self.view.connected = false;
                self.view.status = "outra instância controla o desktop".into();
                self.view.error =
                    Some("o lease do desktop expirou; nenhum novo efeito será enviado".into());
            }
        }
        let mut watchdog_restart = false;
        if let Some(supervisor) = self.supervisor.as_ref() {
            if self.last_heartbeat_sent.elapsed() >= Duration::from_secs(2)
                && supervisor.heartbeat(&Uuid::new_v4().to_string()).is_ok()
            {
                self.last_heartbeat_sent = Instant::now();
            }
            if self.last_heartbeat_received.elapsed() >= Duration::from_secs(6) {
                self.view.connected = false;
                if self.view.active_run.is_none() {
                    self.view.status = "core sem resposta".into();
                }
            }
            watchdog_restart = supervisor.idle_for() >= CORE_UNRESPONSIVE_TIMEOUT;
        }
        if watchdog_restart {
            self.view.error =
                Some("core sem resposta; reiniciando com efeitos em estado desconhecido".into());
            self.restart_core_after_failure();
        }
        let mut events = Vec::new();
        if let Some(supervisor) = self.supervisor.as_ref() {
            while let Some(event) = supervisor.try_event() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                Ok(value) => self.handle_event(value),
                Err(error) => {
                    self.view.error = Some(format!("IPC inválido: {error}"));
                    self.restart_core_after_failure();
                }
            }
        }
        let mut broker_results = Vec::new();
        while let Some(result) = self.broker_worker.try_result() {
            broker_results.push(result);
        }
        for result in broker_results {
            self.handle_broker_result(result);
        }
        self.apply_pending_configuration_if_idle();
        let palette = vox_palette(ui.ctx().theme() == egui::Theme::Dark);
        let panel_margin = if self.view.mode == WindowMode::Compact {
            egui::Margin::symmetric(16, 8)
        } else {
            egui::Margin::same(20)
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(panel_margin))
            .show(ui, |ui| {
                if self.view.mode != WindowMode::Compact {
                    ui.horizontal(|ui| {
                        ui.heading(
                            egui::RichText::new("Vox")
                                .size(18.0)
                                .color(palette.text_primary),
                        );
                        ui.add_space(4.0);
                        let indicator = if self.view.connected { "●" } else { "○" };
                        ui.label(
                            egui::RichText::new(indicator).color(if self.view.connected {
                                palette.success
                            } else {
                                palette.warning
                            }),
                        );
                        ui.label(&self.view.status);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let compact_button = ui.button("Recolher").on_hover_text(
                                "Mantém o Vox visível em modo compacto. Expandir e Parar continuam acessíveis.",
                            );
                            if compact_button.clicked() && self.flush_draft_persist(true) {
                                self.view.request_compact_accompaniment();
                            }
                            ui.menu_button("Mais", |ui| {
                                if ui.button("Nova conversa").clicked() {
                                    self.new_conversation();
                                }
                                if ui.button("Histórico").clicked() {
                                    self.show_history = true;
                                }
                                if ui.button("Preferências").clicked() {
                                    self.show_preferences = true;
                                }
                                if ui.checkbox(&mut self.keep_on_top, "Manter acima").changed() {
                                    self.save_bool_preference("keep_on_top", self.keep_on_top);
                                    ui.ctx()
                                        .send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                                            if self.keep_on_top {
                                                egui::WindowLevel::AlwaysOnTop
                                            } else {
                                                egui::WindowLevel::Normal
                                            },
                                        ));
                                }
                                if ui.button("Sair").clicked() {
                                    if self.view.active_run.is_some() {
                                        self.stop();
                                    }
                                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            });
                        });
                    });
                }
                if let Some(warning) = self.storage_warning.as_deref() {
                    ui.colored_label(palette.warning, warning);
                }
                if self.provider_is_configured {
                    ui.colored_label(
                        palette.warning,
                        "Provider configurado: o texto pode sair deste computador.",
                    );
                }

                if self.view.mode == WindowMode::Compact {
                    let compact_status = self
                        .view
                        .activity
                        .clone()
                        .unwrap_or_else(|| self.view.status.clone());
                    ui.horizontal(|ui| {
                        ui.strong(compact_status);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.view.active_run.is_some() && ui.add(stop_button(palette)).clicked() {
                                self.stop();
                            }
                            if ui.button("Expandir").clicked() {
                                self.view.toggle_compact();
                            }
                        });
                    });
                    if self.view.active_run.is_some() {
                        ui.small(
                            "A janela continua visível. Use Expandir para voltar à conversa ou Parar para interromper a tarefa.",
                        );
                    } else {
                        ui.small(
                            "A janela continua visível. Use Expandir para voltar à conversa e ver o resultado.",
                        );
                    }
                } else {
                    ui.add_space(12.0);
                    if self.view.mode == WindowMode::Approval {
                        ui.colored_label(
                            palette.warning,
                            "Ação aguardando confirmação",
                        );
                        let approval_summary = self
                            .pending_approval_summary
                            .clone()
                            .unwrap_or_else(|| "Esta ação pode alterar o computador.".into());
                        let approval_details = self.pending_approval.as_ref().map(|call| {
                            (
                                call.tool.clone(),
                                serde_json::to_string(&redacted_value_for_display(
                                    &call.arguments,
                                ))
                                .unwrap_or_else(|_| "argumentos indisponíveis".into()),
                                approval_decision_labels(call),
                            )
                        });
                        if let Some((tool, arguments, (approve_label, deny_label))) = approval_details {
                            ui.group(|ui| {
                                ui.strong(&approval_summary);
                                ui.small("Confirme somente se a ação e o alvo correspondem ao seu pedido.");
                                ui.collapsing("Detalhes", |ui| {
                                    ui.label(format!("Ferramenta: {tool}"));
                                    ui.monospace(arguments);
                                });
                                ui.small("A autorização vale somente para esta chamada exata e expira após uso.");
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    if ui.add(primary_button(approve_label, palette)).clicked() {
                                        self.approve_pending();
                                    }
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(deny_label)
                                                    .color(palette.error),
                                            )
                                            .stroke(egui::Stroke::new(1.0, palette.error))
                                            .corner_radius(8)
                                            .min_size(egui::vec2(88.0, 36.0)),
                                        )
                                        .clicked()
                                    {
                                        self.deny_pending();
                                    }
                                });
                            });
                        } else {
                            ui.small(
                                "A confirmação ainda está sendo preparada. Nenhuma ação será executada antes de você ver os detalhes.",
                            );
                        }
                    }
                    let has_model_activity = !self.view.model_activity.is_empty();
                    if self.view.messages.is_empty() {
                        ui.label(egui::RichText::new("Como posso ajudar?").size(15.0));
                        ui.add_space(8.0);
                    } else {
                        let activity_reserve = if self.view.activity.is_some() || has_model_activity {
                            48.0
                        } else {
                            0.0
                        };
                        let model_activity_reserve = if self.view.show_activity && has_model_activity {
                            220.0
                        } else {
                            0.0
                        };
                        let error_reserve = if self.view.error.is_some() { 28.0 } else { 0.0 };
                        let composer_reserve = 150.0;
                        let history_height = (ui.available_height()
                            - activity_reserve
                            - model_activity_reserve
                            - error_reserve
                            - composer_reserve)
                            .max(0.0);
                        egui::ScrollArea::vertical()
                            .id_salt("conversation")
                            .stick_to_bottom(true)
                            .auto_shrink([false, false])
                            .max_height(history_height)
                            .show(ui, |ui| {
                            for message in &self.view.messages {
                                ui.group(|ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.strong(format!("{}:", message.role));
                                        ui.label(&message.content);
                                    });
                                });
                                ui.add_space(8.0);
                            }
                            });
                    }

                    if let Some(activity) = self.view.activity.clone() {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(activity).strong());
                            if self.view.active_run.is_some() && ui.add(stop_button(palette)).clicked() {
                                self.stop();
                            }
                            if ui
                                .button(if self.view.show_activity {
                                    "Ocultar atividade"
                                } else {
                                    "Ver atividade"
                                })
                                .clicked()
                            {
                                self.view.show_activity = !self.view.show_activity;
                            }
                        });
                    } else if has_model_activity {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.small("Solicitação ao modelo registrada com redação de segredos.");
                            if ui
                                .button(if self.view.show_activity {
                                    "Ocultar atividade"
                                } else {
                                    "Ver atividade"
                                })
                                .clicked()
                            {
                                self.view.show_activity = !self.view.show_activity;
                            }
                        });
                    }
                    if self.view.show_activity && has_model_activity {
                        ui.add_space(8.0);
                        ui.group(|ui| {
                            ui.strong("Atividade do modelo");
                            ui.small(
                                "Estas são as mensagens redigidas que saíram do Vox para o provedor. Dados de ferramentas e credenciais sensíveis são ocultados.",
                            );
                            egui::ScrollArea::vertical()
                                .id_salt("model-activity")
                                .max_height(180.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for activity in &self.view.model_activity {
                                        let target = activity
                                            .model_ref
                                            .as_deref()
                                            .map(|model| format!("{} · {}", activity.provider, model))
                                            .unwrap_or_else(|| activity.provider.clone());
                                        ui.label(format!("Rodada {} · {}", activity.round, target));
                                        for message in &activity.messages {
                                            let label = match message.role.as_str() {
                                                "system" => "Instruções do Vox",
                                                "assistant" => "Resposta anterior do modelo",
                                                _ => "Mensagem ou resultado enviado",
                                            };
                                            ui.small(label);
                                            ui.monospace(&message.content);
                                            ui.add_space(6.0);
                                        }
                                        ui.separator();
                                    }
                                });
                        });
                    }
                    if let Some(error) = self.view.error.clone() {
                        ui.colored_label(palette.error, error);
                    }
                    ui.separator();
                    let message_label = ui.label("Mensagem");
                    let response = ui.add_sized(
                        [ui.available_width(), 72.0],
                        egui::TextEdit::multiline(&mut self.view.draft)
                            .hint_text("Peça algo ao seu computador")
                            .desired_rows(3),
                    )
                    .labelled_by(message_label.id);
                    if response.changed() {
                        self.schedule_draft_persist();
                    }
                    let enter = response.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(egui::Key::Enter) && !input.modifiers.shift
                        });
                    let can_submit_draft = self.view.connected
                        && draft_submission_allowed(
                            &self.view,
                            self.pending_approval.is_some(),
                        );
                    ui.horizontal(|ui| {
                        ui.add_enabled(
                            self.voice.is_enabled(),
                            egui::Button::new(self.voice.button_label())
                                .min_size(egui::vec2(44.0, 36.0))
                                .corner_radius(8),
                        )
                        .on_hover_text(self.voice.summary());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let active = self.view.active_run.is_some();
                            if active {
                                if ui.add(stop_button(palette)).clicked() {
                                    self.stop();
                                }
                            } else if self.view.mode != WindowMode::Approval
                                && self.pending_approval.is_none()
                                && ui
                                    .add_enabled(
                                        can_submit_draft && !self.view.draft.trim().is_empty(),
                                        primary_button("Enviar", palette),
                                    )
                                    .clicked()
                            {
                                self.send();
                            }
                        });
                    });
                    ui.label(
                        egui::RichText::new(&self.view.model_label)
                            .small()
                            .color(palette.text_secondary),
                    );
                    if !self.voice.is_enabled() {
                        ui.label(
                            egui::RichText::new(self.voice.summary())
                                .small()
                                .color(palette.text_secondary),
                        );
                    }
                    if enter && can_submit_draft {
                        self.send();
                    }
                    if self.view.active_run.is_some() {
                        ui.small(
                            "Pare a tarefa para enviar outro pedido. Seu rascunho fica preservado.",
                        );
                    }
                }
            });
        if self.show_history {
            let history = self
                .store
                .as_ref()
                .and_then(|store| {
                    store
                        .search_sessions(&self.history_query, 20, self.history_offset)
                        .ok()
                })
                .unwrap_or_default();
            let mut open = true;
            let mut selected_session = None;
            let mut delete_request = None;
            egui::Window::new("Histórico")
                .open(&mut open)
                .resizable(true)
                .show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Buscar");
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.history_query)
                                    .hint_text("título da conversa"),
                            )
                            .changed()
                        {
                            self.history_offset = 0;
                        }
                    });
                    if history.is_empty() {
                        ui.label("Nenhuma conversa encontrada.");
                    }
                    for session in &history {
                        ui.horizontal(|ui| {
                            if ui.button(&session.title).clicked() {
                                selected_session = Some(session.id.clone());
                            }
                            ui.small(&session.updated_at);
                            if session.id == self.session_id {
                                ui.small("atual");
                            }
                            ui.small(format!(
                                "estado: {}",
                                session
                                    .latest_run_state
                                    .as_deref()
                                    .unwrap_or("sem execução")
                            ));
                            if ui.button("Excluir").clicked() {
                                delete_request = Some(session.id.clone());
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(self.history_offset > 0, egui::Button::new("Anterior"))
                            .clicked()
                        {
                            self.history_offset = self.history_offset.saturating_sub(20);
                        }
                        if ui
                            .add_enabled(history.len() == 20, egui::Button::new("Próxima"))
                            .clicked()
                        {
                            self.history_offset += 20;
                        }
                    });
                    if ui.button("Copiar exportação redigida").clicked() {
                        if let Some(store) = self.store.as_ref() {
                            if let Ok(export) = store.export_session(&self.session_id) {
                                ui.copy_text(export);
                            }
                        }
                    }
                    ui.small("A exclusão de uma conversa ativa é bloqueada pelo armazenamento.");
                });
            self.show_history = open;
            if let Some(session_id) = selected_session {
                self.open_session(session_id);
            }
            if let Some(session_id) = delete_request {
                self.delete_confirmation = Some(session_id);
            }
        }
        if let Some(session_id) = self.delete_confirmation.clone() {
            let mut open = true;
            egui::Window::new("Confirmar exclusão")
                .open(&mut open)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(
                        "Excluir esta conversa e suas mensagens? Esta ação não pode ser desfeita.",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Excluir conversa").clicked() {
                            self.delete_session(&session_id);
                        }
                        if ui.button("Cancelar").clicked() {
                            self.delete_confirmation = None;
                        }
                    });
                });
            if !open {
                self.delete_confirmation = None;
            }
        }
        if self.show_preferences {
            let mut open = true;
            egui::Window::new("Preferências")
                .open(&mut open)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label("Tema");
                    let theme_changed = ui
                        .horizontal(|ui| {
                            let mut changed = false;
                            changed |= ui
                                .radio_value(
                                    &mut self.theme_preference,
                                    ThemePreference::System,
                                    "Sistema",
                                )
                                .changed();
                            changed |= ui
                                .radio_value(
                                    &mut self.theme_preference,
                                    ThemePreference::Light,
                                    "Claro",
                                )
                                .changed();
                            changed |= ui
                                .radio_value(
                                    &mut self.theme_preference,
                                    ThemePreference::Dark,
                                    "Escuro",
                                )
                                .changed();
                            changed
                        })
                        .inner;
                    if theme_changed {
                        self.save_theme_preference();
                        ui.ctx().set_theme(self.theme_preference.as_egui());
                        ui.ctx().request_repaint();
                    }
                    if self.theme_preference == ThemePreference::System {
                        ui.small("Segue o tema do sistema quando ele estiver disponível.");
                    }
                    ui.horizontal(|ui| {
                        ui.label("Reter (dias; 0 = não apagar)");
                        let retention_changed = ui
                            .add(
                                egui::TextEdit::singleline(&mut self.retention_input)
                                    .desired_width(72.0),
                            )
                            .changed();
                        if retention_changed {
                            if let Ok(days) = self.retention_input.parse::<u32>() {
                                self.retention_days = days.min(3650);
                                if let Some(store) = self.store.as_ref() {
                                    let _ = store.set_preference(
                                        "retention_days",
                                        &self.retention_days.to_string(),
                                        1,
                                    );
                                }
                            }
                        }
                        if ui.button("Aplicar agora").clicked() && self.retention_days > 0 {
                            if let Some(store) = self.store.as_ref() {
                                if let Ok(removed) = store.prune_sessions_except(
                                    Duration::from_secs(self.retention_days as u64 * 86_400),
                                    Some(&self.session_id),
                                ) {
                                    self.view.status =
                                        format!("{removed} conversa(s) antiga(s) removida(s)");
                                }
                            }
                        }
                    });
                    ui.label("Provider");
                    ui.label(&self.view.model_label);
                    ui.label("Endpoint (HTTPS; HTTP somente em localhost)");
                    ui.text_edit_singleline(&mut self.provider_endpoint_input);
                    ui.label("Modelo");
                    ui.text_edit_singleline(&mut self.provider_model_input);
                    if ui.button("Salvar configuração do provider").clicked() {
                        self.save_provider_settings();
                    }
                    ui.label("Conta do keyring (a chave não fica no SQLite)");
                    ui.text_edit_singleline(&mut self.provider_account);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.provider_secret_input)
                            .password(true)
                            .hint_text("nova chave, opcional"),
                    );
                    if ui.button("Salvar chave no keyring").clicked() {
                        self.save_provider_credential();
                    }
                    if let Some(status) = self.provider_keyring_status.as_deref() {
                        ui.small(status);
                    }
                    ui.small("A configuração é aplicada ao reiniciar o core privado ocioso; durante uma tarefa ela aguarda o término.");
                    ui.separator();
                    ui.label("Pastas permitidas");
                    ui.small(
                        "Uma pasta por linha. O Vox só pode ler, criar, mover ou abrir arquivos dentro destas pastas; diretórios sensíveis fora da lista permanecem bloqueados.",
                    );
                    ui.add_sized(
                        [ui.available_width(), 84.0],
                        egui::TextEdit::multiline(&mut self.file_roots_input)
                            .hint_text("/caminho/para/uma/pasta")
                            .desired_rows(4),
                    );
                    if ui.button("Aplicar pastas permitidas").clicked() {
                        self.save_allowed_roots();
                    }
                    if let Some(status) = self.file_roots_status.as_deref() {
                        ui.small(status);
                    }
                    ui.separator();
                    ui.label("Acessibilidade: depende da permissão do sistema.");
                    ui.label(self.voice.summary());
                    ui.small(self.voice.detail());
                    ui.label(format!("Core conectado: {}", self.view.connected));
                });
            self.show_preferences = open;
        }
        self.sync_viewport(ui.ctx());
        self.flush_draft_persist(false);
        // eframe has no receiver callback to wake this UI for the private
        // core/worker channels, so it polls. Keep the fast cadence only while
        // a run can stream or an effect is active; an idle window must not
        // render an animation loop merely to keep the heartbeat alive.
        let repaint_after = if self.active_broker_execution.is_some() {
            Duration::from_millis(50)
        } else if self.view.active_run.is_some() || self.draft_persist_pending {
            Duration::from_millis(100)
        } else {
            Duration::from_millis(750)
        };
        ui.ctx().request_repaint_after(repaint_after);
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Vox")
            .with_inner_size(INVOCATION_VIEWPORT_SIZE)
            // The compact accompaniment temporarily lowers this bound in
            // `sync_viewport`; ordinary conversation windows keep their
            // composer fully visible.
            .with_min_inner_size(NORMAL_MIN_INNER_SIZE),
        ..Default::default()
    };
    eframe::run_native(
        "Vox",
        options,
        Box::new(|creation_context| {
            let app = VoxApp::new();
            install_vox_theme(&creation_context.egui_ctx, app.theme_preference);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparency_redacts_text_and_structured_arguments_before_rendering() {
        let text = redact_activity_text(
            "Bearer secret-one API_KEY=secret-two sk-or-v1-0123456789abcdef0123456789abcdef",
        );
        assert!(!text.contains("secret-one"));
        assert!(!text.contains("secret-two"));
        assert!(!text.contains("sk-or-v1-0123456789abcdef0123456789abcdef"));
        assert!(text.contains("[REDACTED]"));

        let value = redacted_value_for_display(&serde_json::json!({
            "authorization": "Bearer hidden",
            "nested": {"token": "also-hidden", "safe": "visible"}
        }));
        let rendered = value.to_string();
        assert!(!rendered.contains("hidden"));
        assert!(!rendered.contains("also-hidden"));
        assert!(rendered.contains("visible"));
    }

    #[test]
    fn transparency_redacts_portuguese_credential_names_before_rendering() {
        let text =
            redact_activity_text("Senha=segredo-um credencial: segredo-dois chave=segredo-tres");
        assert!(!text.contains("segredo-um"));
        assert!(!text.contains("segredo-dois"));
        assert!(!text.contains("segredo-tres"));

        let value = redacted_value_for_display(&serde_json::json!({
            "senha": "oculto-um",
            "nested": {
                "credenciais": "oculto-dois",
                "chave": "oculto-tres",
                "safe": "visível"
            }
        }));
        let rendered = value.to_string();
        assert!(!rendered.contains("oculto-um"));
        assert!(!rendered.contains("oculto-dois"));
        assert!(!rendered.contains("oculto-tres"));
        assert!(rendered.contains("visível"));
    }

    #[test]
    fn transparency_fully_redacts_quoted_credential_values_with_spaces() {
        let redacted = redact_activity_text(
            "password=\"english secret with spaces\" senha='segredo em portugues com espacos'",
        );
        assert!(!redacted.contains("english secret with spaces"));
        assert!(!redacted.contains("segredo em portugues com espacos"));
        assert_eq!(redacted, "password=\"[REDACTED]\" senha='[REDACTED]'");
    }

    #[test]
    fn theme_preference_defaults_to_system_and_migrates_legacy_choice() {
        assert_eq!(
            ThemePreference::from_preferences(None, None),
            ThemePreference::System
        );
        assert_eq!(
            ThemePreference::from_preferences(Some("dark"), Some("false")),
            ThemePreference::Dark
        );
        assert_eq!(
            ThemePreference::from_preferences(None, Some("true")),
            ThemePreference::Dark
        );
        assert_eq!(
            ThemePreference::from_preferences(None, Some("false")),
            ThemePreference::Light
        );
        assert_eq!(ThemePreference::System.storage_value(), "system");
        assert_eq!(
            ThemePreference::Light.as_egui(),
            egui::ThemePreference::Light
        );
    }

    #[test]
    fn system_theme_uses_the_operating_system_and_light_fallback() {
        let ctx = egui::Context::default();
        install_vox_theme(&ctx, ThemePreference::System);
        assert_eq!(ctx.theme(), egui::Theme::Light);

        ctx.begin_pass(egui::RawInput {
            system_theme: Some(egui::Theme::Dark),
            ..Default::default()
        });
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        let mut output = ctx.end_pass();
        output.textures_delta.clear();

        ctx.begin_pass(egui::RawInput {
            system_theme: Some(egui::Theme::Light),
            ..Default::default()
        });
        assert_eq!(ctx.theme(), egui::Theme::Light);
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
    }

    #[test]
    fn approval_controls_name_the_effect_and_draft_waits_for_a_decision() {
        let overwrite = BrokerCall {
            call_id: "overwrite".into(),
            run_id: "run".into(),
            tool: "files.write".into(),
            arguments: serde_json::json!({"overwrite": true}),
            authorization_ref: Some("approval".into()),
        };
        assert_eq!(
            approval_decision_labels(&overwrite),
            ("Substituir arquivo", "Cancelar alteração")
        );
        let shell = BrokerCall {
            tool: "shell.exec".into(),
            ..overwrite.clone()
        };
        assert_eq!(
            approval_decision_labels(&shell),
            ("Executar comando", "Cancelar comando")
        );

        let mut view = PresentationState::default();
        assert!(draft_submission_allowed(&view, false));
        view.mode = WindowMode::Approval;
        assert!(!draft_submission_allowed(&view, false));
        view.mode = WindowMode::Conversation;
        assert!(!draft_submission_allowed(&view, true));
        view.active_run = Some("run".into());
        assert!(!draft_submission_allowed(&view, false));
    }

    #[test]
    fn saved_provider_fields_select_the_private_provider_and_default_to_safe_demo() {
        let demo = provider_core_environment("", "");
        assert!(demo.contains(&("VOX_PROVIDER".into(), "fake".into())));
        assert!(demo.contains(&("VOX_PROVIDER_API_KEY".into(), String::new())));
        assert!(!provider_is_configured("", "model"));

        let configured = provider_core_environment("http://127.0.0.1:8080", "local-model");
        assert!(configured.contains(&("VOX_PROVIDER".into(), "openai-compatible".into())));
        assert!(configured.contains(&(
            "VOX_PROVIDER_BASE_URL".into(),
            "http://127.0.0.1:8080".into()
        )));
        assert!(provider_is_configured(
            "http://127.0.0.1:8080",
            "local-model"
        ));
    }

    #[test]
    fn provider_configuration_never_treats_a_plaintext_key_as_metadata() {
        let provider_key = "sk-or-v1-0123456789abcdef0123456789abcdef";
        assert!(provider_configuration_contains_secret(provider_key));
        assert!(!provider_configuration_is_safe(
            "https://provider.example/v1",
            provider_key
        ));
        assert!(provider_keyring_environment(provider_key, true).is_empty());

        let environment = provider_core_environment("https://provider.example/v1", provider_key);
        assert!(environment.contains(&("VOX_PROVIDER".into(), "fake".into())));
        assert!(environment.contains(&("VOX_PROVIDER_API_KEY".into(), String::new())));
    }

    #[test]
    fn allowed_roots_require_existing_directories_and_are_canonicalized() {
        let root = std::env::temp_dir().join(format!("vox-allowed-roots-{}", Uuid::new_v4()));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();

        let input = format!("{}\n{}", root.display(), root.join(".").display());
        let roots = parse_allowed_roots(&input).unwrap();
        assert_eq!(roots, vec![root.canonicalize().unwrap()]);
        assert!(parse_allowed_roots(&root.join("missing").display().to_string()).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn queued_cancellation_keeps_an_approval_unconsumed() {
        let broker = Arc::new(Broker::new(RuntimeConfig::default()));
        let worker = BrokerWorker::spawn(Arc::clone(&broker));
        let mut call = BrokerCall {
            call_id: "cancel-before-start".into(),
            run_id: "run-cancel-before-start".into(),
            tool: "shell.exec".into(),
            arguments: serde_json::json!({"program":"printf","argv":["still-approved"]}),
            authorization_ref: None,
        };
        let approval = broker.request_approval(&call).unwrap();
        call.authorization_ref = Some(approval.approval_id);
        let cancel = Arc::new(AtomicBool::new(true));

        worker
            .execute(
                "cancel-before-start-job".into(),
                call.clone(),
                Arc::clone(&cancel),
            )
            .unwrap();
        let result = worker.recv_timeout(Duration::from_secs(1)).unwrap();

        assert!(result.cancel_requested);
        assert!(result.result.is_err());
        // The worker checks cancellation before entering the broker, so the
        // authorization remains available for the exact call it was issued for.
        assert_eq!(broker.execute(&call).unwrap().status, "success");
    }

    #[cfg(unix)]
    #[test]
    fn running_worker_execution_observes_cancellation() {
        let broker = Arc::new(Broker::new(RuntimeConfig::default()));
        let worker = BrokerWorker::spawn(Arc::clone(&broker));
        let mut call = BrokerCall {
            call_id: "cancel-running".into(),
            run_id: "run-cancel-running".into(),
            tool: "shell.exec".into(),
            arguments: serde_json::json!({
                "program":"sh",
                "argv":["-c","sleep 30"],
                "timeout_ms": 30_000
            }),
            authorization_ref: None,
        };
        let approval = broker.request_approval(&call).unwrap();
        call.authorization_ref = Some(approval.approval_id);
        let cancel = Arc::new(AtomicBool::new(false));

        worker
            .execute("cancel-running-job".into(), call, Arc::clone(&cancel))
            .unwrap();
        std::thread::sleep(Duration::from_millis(75));
        cancel.store(true, Ordering::Relaxed);
        let result = worker.recv_timeout(Duration::from_secs(2)).unwrap();

        assert!(result.cancel_requested);
        assert!(matches!(result.result, Err(message) if message.contains("cancelled")));
    }
}

mod ui_state;

use eframe::egui;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use ui_state::{PresentationState, WindowMode};
use uuid::Uuid;
use vox_broker::{Broker, BrokerCall};
use vox_desktop_access::MockDesktop;
use vox_secrets::SecretStore;
use vox_session_store::{ApprovalRecord, EffectRecord, SessionStore};
use vox_supervisor::{default_entry, default_node, AgentSupervisor};
use vox_tool_runtime::RuntimeConfig;

fn provider_keyring_environment(configured_account: &str) -> Vec<(String, String)> {
    if std::env::var("VOX_PROVIDER").as_deref() != Ok("openai-compatible") {
        return Vec::new();
    }
    if std::env::var_os("VOX_PROVIDER_API_KEY").is_some() {
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
    if std::env::var("VOX_PROVIDER").as_deref() != Ok("openai-compatible") {
        return Vec::new();
    }
    vec![
        ("VOX_PROVIDER_BASE_URL".into(), base_url.to_owned()),
        ("VOX_PROVIDER_MODEL".into(), model.to_owned()),
    ]
}

struct VoxApp {
    supervisor: Option<AgentSupervisor>,
    session_id: String,
    view: PresentationState,
    fixture: MockDesktop,
    store: Option<SessionStore>,
    storage_warning: Option<String>,
    broker: Broker,
    keep_on_top: bool,
    dark_theme: bool,
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
    active_store_run: Option<String>,
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
    configuration_restart_pending: bool,
    desktop_lease_owner: Option<String>,
    last_lease_renewed: Instant,
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
        let dark_theme = store
            .as_ref()
            .and_then(|store| store.preference("dark_theme").ok().flatten())
            .map(|value| value == "true")
            .unwrap_or(false);
        let retention_days = store
            .as_ref()
            .and_then(|store| store.preference("retention_days").ok().flatten())
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        let provider_is_configured =
            std::env::var("VOX_PROVIDER").as_deref() == Ok("openai-compatible");
        let provider_account = store
            .as_ref()
            .and_then(|store| store.preference("provider_account").ok().flatten())
            .or_else(|| std::env::var("VOX_PROVIDER_ACCOUNT").ok())
            .unwrap_or_default();
        let provider_endpoint_input = store
            .as_ref()
            .and_then(|store| store.preference("provider_base_url").ok().flatten())
            .or_else(|| std::env::var("VOX_PROVIDER_BASE_URL").ok())
            .unwrap_or_default();
        let provider_model_input = store
            .as_ref()
            .and_then(|store| store.preference("provider_model").ok().flatten())
            .or_else(|| std::env::var("VOX_PROVIDER_MODEL").ok())
            .unwrap_or_default();
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
            "Modelo de referência · Local".into()
        };
        let session_id = if can_start_core {
            store
                .as_ref()
                .and_then(|store| store.create_session("Vox").ok())
                .unwrap_or(provisional_session_id)
        } else {
            provisional_session_id
        };
        let runtime_config = RuntimeConfig {
            xa11y_command: std::env::var_os("VOX_XA11Y_BIN").map(std::path::PathBuf::from),
            ..RuntimeConfig::default()
        };
        let mut core_environment =
            provider_core_environment(&provider_endpoint_input, &provider_model_input);
        core_environment.extend(provider_keyring_environment(&provider_account));
        let mut app = Self {
            supervisor: None,
            session_id,
            view: PresentationState::default(),
            fixture: MockDesktop::fixture(),
            store,
            storage_warning,
            broker: Broker::new(runtime_config),
            keep_on_top,
            dark_theme,
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
            active_store_run: None,
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
            configuration_restart_pending: false,
            desktop_lease_owner,
            last_lease_renewed: Instant::now(),
        };
        app.view.model_label = model_label;
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
                    app.view.connected = initialized && opened;
                    app.view.status = if app.view.connected {
                        "conectado ao core"
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
        let size = match self.view.mode {
            WindowMode::Invocation => [440.0, 176.0],
            WindowMode::Conversation | WindowMode::Approval => [440.0, 520.0],
            WindowMode::Compact => [360.0, 88.0],
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            size[0], size[1],
        )));
        self.last_viewport_mode = self.view.mode;
    }

    fn send(&mut self) {
        let content = self.view.draft.trim().to_owned();
        if content.is_empty() || self.view.active_run.is_some() || !self.view.connected {
            return;
        }
        let Some(supervisor) = self.supervisor.as_ref() else {
            return;
        };
        let run_id = Uuid::new_v4().to_string();
        self.view.submit(content.clone(), run_id.clone());
        if let Some(store) = self.store.as_ref() {
            self.active_store_run = store
                .start_run(&self.session_id, Some(&self.view.model_label))
                .ok()
                .map(|run| run.id);
            let _ = store.append_message(
                &self.session_id,
                "user",
                &content,
                self.view.messages.len() as i64,
            );
        }
        self.view.draft.clear();
        if let Err(error) = supervisor.start_turn(
            &Uuid::new_v4().to_string(),
            &self.session_id,
            &run_id,
            &content,
        ) {
            self.view.fail(format!("falha ao enviar: {error}"));
            self.finish_store_run("failed");
        }
    }

    fn open_session(&mut self, session_id: String) {
        if self.view.active_run.is_some() || self.session_id == session_id {
            return;
        }
        let messages = self
            .store
            .as_ref()
            .and_then(|store| store.messages(&session_id).ok())
            .unwrap_or_default();
        self.session_id = session_id.clone();
        self.view.messages = messages
            .into_iter()
            .map(|message| ui_state::UiMessage {
                role: if message.role == "assistant" {
                    "Vox".into()
                } else {
                    "Você".into()
                },
                content: message.content,
            })
            .collect();
        self.view.draft.clear();
        self.view.error = None;
        self.view.activity = None;
        self.view.status = "sessão carregada".into();
        if let Some(supervisor) = self.supervisor.as_ref() {
            let _ = supervisor.open_session(&Uuid::new_v4().to_string(), &session_id);
        }
    }

    fn new_conversation(&mut self) {
        if self.view.active_run.is_some() {
            return;
        }
        let Some(store) = self.store.as_ref() else {
            self.view.messages.clear();
            self.view.draft.clear();
            return;
        };
        let Ok(session_id) = store.create_session("Nova conversa") else {
            self.view.fail("não foi possível criar uma nova conversa");
            return;
        };
        self.session_id = session_id.clone();
        self.view.messages.clear();
        self.view.draft.clear();
        self.view.error = None;
        self.view.activity = None;
        self.view.status = "nova conversa".into();
        if let Some(supervisor) = self.supervisor.as_ref() {
            let _ = supervisor.open_session(&Uuid::new_v4().to_string(), &session_id);
        }
    }

    fn delete_session(&mut self, session_id: &str) {
        let result = self
            .store
            .as_ref()
            .map(|store| store.delete_session(session_id));
        match result {
            Some(Ok(())) if session_id == self.session_id => self.new_conversation(),
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

    fn save_provider_credential(&mut self) {
        let account = self.provider_account.trim().to_owned();
        if account.is_empty() {
            self.provider_keyring_status = Some("Informe uma conta para o keyring.".into());
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
        let mut environment =
            provider_core_environment(&self.provider_endpoint_input, &self.provider_model_input);
        environment.extend(provider_keyring_environment(&self.provider_account));
        self.core_environment = environment;
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

    fn effect_envelope(
        data: &Value,
        error_code: Option<&str>,
        error: Option<&str>,
        verification: Option<&Value>,
    ) -> Value {
        serde_json::json!({
            "data": data,
            "error_code": error_code,
            "error": error,
            "verification": verification,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_effect(
        &self,
        call: &BrokerCall,
        status: &str,
        side_effect: &str,
        data: &Value,
        error_code: Option<&str>,
        error: Option<&str>,
        verification: Option<&Value>,
    ) {
        if let Some(store) = self.store.as_ref() {
            let _ = store.complete_effect(
                &Self::effect_key(call),
                status,
                side_effect,
                &Self::effect_envelope(data, error_code, error, verification),
            );
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

    fn revoke_pending_approval(&mut self, state: &str) {
        let Some(call) = self.pending_approval.take() else {
            return;
        };
        if let Some(approval_id) = call.authorization_ref.as_deref() {
            let _ = self.broker.revoke_approval(approval_id);
        }
        self.record_approval_state(&call, state);
        if state == "cancelled" {
            self.complete_effect(
                &call,
                "cancelled",
                "none",
                &Value::Null,
                Some("CANCELLED_BEFORE_EFFECT"),
                Some("ação cancelada antes da execução"),
                None,
            );
        }
    }

    fn stop(&mut self) {
        self.revoke_pending_approval("cancelled");
        let Some(run_id) = self.view.active_run.clone() else {
            return;
        };
        if let Some(supervisor) = self.supervisor.as_ref() {
            let _ = supervisor.cancel(&Uuid::new_v4().to_string(), &run_id);
            self.view.status = "parando…".into();
        }
    }

    fn restart_core_after_failure(&mut self) {
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
                self.view.connected = initialized && opened;
                self.view.status = if self.view.connected {
                    "core reiniciado; pronto".into()
                } else {
                    "core reiniciado, handshake pendente".into()
                };
                if configuration_was_pending && self.view.connected {
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
                self.view.connected = initialized && opened;
                self.view.status = if self.view.connected {
                    "configuração aplicada; core pronto".into()
                } else {
                    "core reiniciado, handshake pendente".into()
                };
                self.supervisor = Some(supervisor);
                self.view.connected
            }
            Err(error) => {
                self.view
                    .fail(format!("não foi possível aplicar configuração: {error}"));
                false
            }
        }
    }

    fn apply_pending_configuration_if_idle(&mut self) {
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
        let Some(call) = self.pending_approval.clone() else {
            return;
        };
        let result = match self.broker.execute(&call) {
            Ok(result) => result,
            Err(error) => {
                let message = error.to_string();
                self.complete_effect(
                    &call,
                    "error",
                    "none",
                    &Value::Null,
                    Some("BROKER_ERROR"),
                    Some(&message),
                    None,
                );
                if let Some(supervisor) = self.supervisor.as_ref() {
                    let _ = supervisor.send_tool_result_detailed(
                        &call.run_id,
                        &call.call_id,
                        &call.tool,
                        "error",
                        "none",
                        &Value::Null,
                        Some("BROKER_ERROR"),
                        Some(&message),
                    );
                }
                self.record_approval_state(&call, "invalidated");
                self.view.fail(format!("falha ao autorizar: {error}"));
                self.pending_approval = None;
                return;
            }
        };
        self.complete_effect(
            &call,
            &result.status,
            &result.side_effect,
            &result.data,
            result.error_code.as_deref(),
            result.error.as_deref(),
            result.verification.as_ref(),
        );
        if let Some(supervisor) = self.supervisor.as_ref() {
            let _ = supervisor.send_tool_result_with_verification(
                &call.run_id,
                &call.call_id,
                &call.tool,
                &result.status,
                &result.side_effect,
                &result.data,
                result.verification.as_ref(),
                result.error_code.as_deref(),
                result.error.as_deref(),
            );
        }
        self.record_approval_state(&call, "consumed");
        self.pending_approval = None;
        self.view.mode = WindowMode::Conversation;
        self.view.status = "ação autorizada; conferindo resultado".into();
    }

    fn deny_pending(&mut self) {
        let Some(call) = self.pending_approval.take() else {
            return;
        };
        if let Some(approval_id) = call.authorization_ref.as_deref() {
            let _ = self.broker.revoke_approval(approval_id);
        }
        self.record_approval_state(&call, "denied");
        self.complete_effect(
            &call,
            "cancelled",
            "none",
            &Value::Null,
            Some("USER_DENIED"),
            Some("ação negada pelo usuário"),
            None,
        );
        if let Some(supervisor) = self.supervisor.as_ref() {
            let _ = supervisor.send_tool_result_detailed(
                &call.run_id,
                &call.call_id,
                &call.tool,
                "cancelled",
                "none",
                &serde_json::Value::Null,
                Some("USER_DENIED"),
                Some("ação negada pelo usuário"),
            );
        }
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
                self.view.connected = true;
                self.view.status = "pronto".into();
                self.last_heartbeat_received = Instant::now();
            }
            "heartbeat" => {
                self.last_heartbeat_received = Instant::now();
                self.view.connected = true;
                if self.view.active_run.is_none() {
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
            "message.delta" => {
                let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                    return;
                };
                let seq = event.get("seq").and_then(Value::as_u64).unwrap_or(0);
                let delta = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.view.append_delta_for(run_id, seq, delta);
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
                self.view.show_activity = true;
                if let (Some(call_id), Some(tool)) = (
                    event.get("call_id").and_then(Value::as_str),
                    event.get("tool").and_then(Value::as_str),
                ) {
                    let call = BrokerCall {
                        call_id: call_id.into(),
                        run_id: run_id.into(),
                        tool: tool.into(),
                        arguments: event
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!({})),
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
                    let broker_result = match self.broker.execute(&call) {
                        Ok(result) => (
                            result.status,
                            result.side_effect,
                            result.data,
                            result.error_code,
                            result.error,
                            result.verification,
                        ),
                        Err(error) => (
                            "error".into(),
                            "none".into(),
                            serde_json::json!({"fixture":self.fixture.snapshot(),"observed":false}),
                            Some("BROKER_ERROR".into()),
                            Some(error.to_string()),
                            None,
                        ),
                    };
                    if broker_result.3.as_deref() == Some("APPROVAL_REQUIRED") {
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
                                self.pending_approval = Some(pending);
                            }
                            Err(error) => {
                                self.complete_effect(
                                    &call,
                                    "error",
                                    "none",
                                    &Value::Null,
                                    Some("BROKER_ERROR"),
                                    Some(&error.to_string()),
                                    None,
                                );
                                if let Some(supervisor) = self.supervisor.as_ref() {
                                    let _ = supervisor.send_tool_result_detailed(
                                        run_id,
                                        call_id,
                                        tool,
                                        "error",
                                        "none",
                                        &Value::Null,
                                        Some("BROKER_ERROR"),
                                        Some(&error.to_string()),
                                    );
                                }
                                self.view.fail_for(
                                    run_id,
                                    format!("não foi possível criar aprovação: {error}"),
                                );
                                return;
                            }
                        }
                        self.view.mode = WindowMode::Approval;
                        self.view.activity = Some("Essa ação altera o computador".into());
                        self.view.status = "aguardando sua confirmação".into();
                        return;
                    }
                    self.complete_effect(
                        &call,
                        &broker_result.0,
                        &broker_result.1,
                        &broker_result.2,
                        broker_result.3.as_deref(),
                        broker_result.4.as_deref(),
                        broker_result.5.as_ref(),
                    );
                    if let Some(supervisor) = self.supervisor.as_ref() {
                        let _ = supervisor.send_tool_result_with_verification(
                            run_id,
                            call_id,
                            tool,
                            &broker_result.0,
                            &broker_result.1,
                            &broker_result.2,
                            broker_result.5.as_ref(),
                            broker_result.3.as_deref(),
                            broker_result.4.as_deref(),
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
                self.view.show_activity = true;
            }
            "run.completed" => {
                let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                    return;
                };
                if self.view.active_run.as_deref() != Some(run_id) {
                    return;
                }
                self.finish_store_run("completed");
                if let Some(content) = event.get("content").and_then(Value::as_str) {
                    self.view.complete_for(run_id, content);
                    if let Some(store) = self.store.as_ref() {
                        let _ = store.append_message(
                            &self.session_id,
                            "assistant",
                            content,
                            self.view.messages.len() as i64,
                        );
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
                self.finish_store_run("cancelled");
                self.view.cancel_for(run_id);
            }
            "run.failed" | "error" => {
                if kind == "run.failed" {
                    let Some(run_id) = event.get("run_id").and_then(Value::as_str) else {
                        return;
                    };
                    if self.view.active_run.as_deref() != Some(run_id) {
                        return;
                    }
                }
                self.finish_store_run("failed");
                let message = event
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("falha");
                if kind == "run.failed" {
                    if let Some(run_id) = event.get("run_id").and_then(Value::as_str) {
                        self.view.fail_for(run_id, message);
                    }
                } else {
                    self.view.fail(message);
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
            watchdog_restart = !supervisor
                .watchdog_expired(Duration::from_secs(12))
                .is_empty();
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
        self.apply_pending_configuration_if_idle();
        ui.ctx().set_visuals(if self.dark_theme {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::same(20)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(egui::RichText::new("Vox").size(18.0));
                    ui.add_space(4.0);
                    let indicator = if self.view.connected { "●" } else { "○" };
                    ui.label(
                        egui::RichText::new(indicator).color(if self.view.connected {
                            egui::Color32::from_rgb(33, 104, 61)
                        } else {
                            egui::Color32::from_rgb(134, 84, 10)
                        }),
                    );
                    ui.label(&self.view.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Ocultar").clicked() {
                            if self.view.active_run.is_some() {
                                self.view.mode = WindowMode::Compact;
                            }
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        ui.menu_button("Mais", |ui| {
                            if ui.button("Nova conversa").clicked() {
                                self.new_conversation();
                            }
                            if ui.button("Recolher / expandir").clicked() {
                                self.view.toggle_compact();
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
                if let Some(warning) = self.storage_warning.as_deref() {
                    ui.colored_label(egui::Color32::from_rgb(134, 84, 10), warning);
                }
                if self.provider_is_configured {
                    ui.colored_label(
                        egui::Color32::from_rgb(134, 84, 10),
                        "Provider configurado: o texto pode sair deste computador.",
                    );
                }

                if self.view.mode == WindowMode::Compact {
                    ui.add_space(16.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(
                            self.view
                                .activity
                                .as_deref()
                                .unwrap_or("Tarefa em andamento"),
                        );
                        if ui.button("Expandir").clicked() {
                            self.view.toggle_compact();
                        }
                        if self.view.active_run.is_some() && ui.button("Parar").clicked() {
                            self.stop();
                        }
                    });
                    ui.label("A janela está recolhida; a tarefa continua sob seu controle.");
                } else {
                    ui.add_space(12.0);
                    if self.view.mode == WindowMode::Approval {
                        ui.colored_label(
                            egui::Color32::from_rgb(134, 84, 10),
                            "Ação aguardando confirmação",
                        );
                        if let Some(call) = self.pending_approval.as_ref() {
                            ui.group(|ui| {
                                ui.label(format!("Ferramenta: {}", call.tool));
                                ui.label(format!("Run: {}", call.run_id));
                                if let Some(approval_id) = call.authorization_ref.as_deref() {
                                    ui.small(format!("Approval: {approval_id}"));
                                }
                                ui.monospace(
                                    serde_json::to_string(&call.arguments)
                                        .unwrap_or_else(|_| "argumentos indisponíveis".into()),
                                );
                                ui.small("A autorização vale somente para esta chamada exata e expira após uso.");
                            });
                        }
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("conversation")
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if self.view.messages.is_empty() {
                                ui.label(egui::RichText::new("Como posso ajudar?").size(15.0));
                            }
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

                    if let Some(activity) = self.view.activity.clone() {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(activity).strong());
                            if self.view.mode == WindowMode::Approval {
                                if ui.button("Autorizar").clicked() {
                                    self.approve_pending();
                                }
                                if ui.button("Cancelar ação").clicked() {
                                    self.deny_pending();
                                }
                            }
                            if self.view.active_run.is_some() && ui.button("Parar").clicked() {
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
                    }
                    if let Some(error) = self.view.error.clone() {
                        ui.colored_label(egui::Color32::from_rgb(180, 35, 53), error);
                    }
                    ui.separator();
                    ui.label("Mensagem");
                    let response = ui.add_sized(
                        [ui.available_width(), 72.0],
                        egui::TextEdit::multiline(&mut self.view.draft)
                            .hint_text("Peça algo ao seu computador")
                            .desired_rows(3),
                    );
                    let enter = response.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(egui::Key::Enter) && !input.modifiers.shift
                        });
                    ui.horizontal(|ui| {
                        ui.label(&self.view.model_label);
                        ui.add_enabled(false, egui::Button::new("Segure para falar"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let active = self.view.active_run.is_some();
                            if active {
                                if ui.button("Parar").clicked() {
                                    self.stop();
                                }
                            } else if ui
                                .add_enabled(
                                    !self.view.draft.trim().is_empty(),
                                    egui::Button::new("Enviar"),
                                )
                                .clicked()
                            {
                                self.send();
                            }
                        });
                    });
                    if enter && self.view.active_run.is_none() {
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
                    if ui.checkbox(&mut self.dark_theme, "Tema escuro").changed() {
                        self.save_bool_preference("dark_theme", self.dark_theme);
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
                    ui.label("Acessibilidade: depende da permissão do sistema.");
                    ui.label("Voz: push-to-talk ainda não configurado nesta versão.");
                    ui.label(format!("Core conectado: {}", self.view.connected));
                });
            self.show_preferences = open;
        }
        self.sync_viewport(ui.ctx());
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Vox")
            .with_inner_size([440.0, 176.0])
            .with_min_inner_size([320.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Vox",
        options,
        Box::new(|_creation_context| Ok(Box::new(VoxApp::new()))),
    )
}

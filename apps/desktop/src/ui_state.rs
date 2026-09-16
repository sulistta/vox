#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    Invocation,
    Conversation,
    Compact,
    Approval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationState {
    pub mode: WindowMode,
    pub messages: Vec<UiMessage>,
    pub draft: String,
    pub active_run: Option<String>,
    pub status: String,
    pub activity: Option<String>,
    pub error: Option<String>,
    pub connected: bool,
    pub show_activity: bool,
    pub model_label: String,
    last_seq: u64,
    terminal: bool,
}

impl Default for PresentationState {
    fn default() -> Self {
        Self {
            mode: WindowMode::Invocation,
            messages: Vec::new(),
            draft: String::new(),
            active_run: None,
            status: "iniciando core privado".into(),
            activity: None,
            error: None,
            connected: false,
            show_activity: false,
            model_label: "Modelo de referência · Local".into(),
            last_seq: 0,
            terminal: false,
        }
    }
}

impl PresentationState {
    pub fn submit(&mut self, content: impl Into<String>, run_id: impl Into<String>) {
        let content = content.into();
        if content.trim().is_empty() || self.active_run.is_some() {
            return;
        }
        self.messages.push(UiMessage {
            role: "Você".into(),
            content,
        });
        self.active_run = Some(run_id.into());
        self.mode = WindowMode::Conversation;
        self.status = "pensando".into();
        self.error = None;
        self.last_seq = 0;
        self.terminal = false;
    }

    pub fn append_delta(&mut self, seq: u64, delta: &str) {
        if self.terminal || seq <= self.last_seq {
            return;
        }
        self.last_seq = seq;
        if let Some(last) = self.messages.last_mut() {
            if last.role == "Vox" {
                last.content.push_str(delta);
                return;
            }
        }
        self.messages.push(UiMessage {
            role: "Vox".into(),
            content: delta.into(),
        });
    }

    pub fn append_delta_for(&mut self, run_id: &str, seq: u64, delta: &str) {
        if self.active_run.as_deref() != Some(run_id) {
            return;
        }
        self.append_delta(seq, delta);
    }

    pub fn complete(&mut self, content: &str) {
        if self.terminal {
            return;
        }
        let already_streamed = self
            .messages
            .last()
            .is_some_and(|message| message.role == "Vox" && message.content == content);
        if !already_streamed && !content.is_empty() {
            self.messages.push(UiMessage {
                role: "Vox".into(),
                content: content.into(),
            });
        }
        self.active_run = None;
        self.status = "concluído".into();
        self.activity = None;
        self.terminal = true;
    }

    pub fn complete_for(&mut self, run_id: &str, content: &str) {
        if self.active_run.as_deref() == Some(run_id) {
            self.complete(content);
        }
    }

    pub fn cancel(&mut self) {
        if self.terminal {
            return;
        }
        self.active_run = None;
        self.status = "cancelado; nenhum novo efeito será enviado".into();
        self.activity = None;
        self.terminal = true;
    }

    pub fn cancel_for(&mut self, run_id: &str) {
        if self.active_run.as_deref() == Some(run_id) {
            self.cancel();
        }
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.error = Some(message.clone());
        self.status = message;
        self.active_run = None;
        self.activity = None;
        self.terminal = true;
    }

    pub fn fail_for(&mut self, run_id: &str, message: impl Into<String>) {
        if self.active_run.as_deref() == Some(run_id) {
            self.fail(message);
        }
    }

    pub fn toggle_compact(&mut self) {
        self.mode = match self.mode {
            WindowMode::Compact => WindowMode::Conversation,
            _ => WindowMode::Compact,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_completion_is_not_duplicated() {
        let mut state = PresentationState::default();
        state.submit("oi", "run");
        state.append_delta(1, "Olá");
        state.complete("Olá");
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.messages[1].content, "Olá");
        assert_eq!(state.active_run, None);
    }

    #[test]
    fn compact_mode_does_not_drop_an_active_run() {
        let mut state = PresentationState::default();
        state.submit("faça", "run");
        state.toggle_compact();
        assert_eq!(state.mode, WindowMode::Compact);
        assert_eq!(state.active_run.as_deref(), Some("run"));
    }

    #[test]
    fn out_of_order_and_duplicate_deltas_do_not_reopen_or_duplicate_state() {
        let mut state = PresentationState::default();
        state.submit("oi", "run");
        state.append_delta(2, "mundo");
        state.append_delta(1, "Olá ");
        state.append_delta(2, "mundo");
        assert_eq!(state.messages[1].content, "mundo");
        state.complete("mundo");
        state.complete("mundo");
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.active_run, None);
    }

    #[test]
    fn a_terminal_run_can_still_surface_a_later_core_failure() {
        let mut state = PresentationState::default();
        state.submit("oi", "run");
        state.complete("ok");
        state.fail("core caiu");
        assert_eq!(state.status, "core caiu");
        assert_eq!(state.error.as_deref(), Some("core caiu"));
        assert_eq!(state.active_run, None);
    }

    #[test]
    fn stale_events_from_an_old_run_cannot_mutate_the_new_run() {
        let mut state = PresentationState::default();
        state.submit("primeiro", "run-old");
        state.complete_for("run-old", "ok");
        state.submit("segundo", "run-new");
        state.append_delta_for("run-old", 1, "evento velho");
        state.complete_for("run-old", "resultado velho");
        state.cancel_for("run-old");
        state.fail_for("run-old", "falha velha");
        assert_eq!(state.active_run.as_deref(), Some("run-new"));
        assert_eq!(
            state
                .messages
                .last()
                .map(|message| message.content.as_str()),
            Some("segundo")
        );
        assert_eq!(state.status, "pensando");
    }

    #[test]
    fn matching_terminal_event_closes_only_its_run() {
        let mut state = PresentationState::default();
        state.submit("oi", "run");
        state.append_delta_for("run", 1, "olá");
        state.complete_for("run", "olá");
        assert_eq!(state.active_run, None);
        assert_eq!(state.status, "concluído");
    }
}

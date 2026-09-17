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

/// A redacted transcript of a request sent to the configured language model.
/// It is retained only in the live presentation state so the user can inspect
/// the current run under “Ver atividade”; it is deliberately separate from
/// chat history and never authorizes a desktop action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelActivityMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelActivityRound {
    pub round: u64,
    pub provider: String,
    pub model_ref: Option<String>,
    pub messages: Vec<ModelActivityMessage>,
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
    pub model_activity: Vec<ModelActivityRound>,
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
            model_activity: Vec::new(),
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
        self.model_activity.clear();
        self.show_activity = false;
        self.last_seq = 0;
        self.terminal = false;
    }

    /// Undo the optimistic local bubble when the private core never accepted
    /// the turn. The draft itself belongs to the composer and is intentionally
    /// left intact so the person can retry without creating a duplicate.
    pub fn withdraw_submission_for(&mut self, run_id: &str) {
        if self.active_run.as_deref() != Some(run_id) {
            return;
        }
        if self
            .messages
            .last()
            .is_some_and(|message| message.role == "Você")
        {
            self.messages.pop();
        }
        self.active_run = None;
        self.activity = None;
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

    /// Records a transparency event only for the active run. A malformed or
    /// stale core event cannot overwrite the user-visible activity of a newer
    /// task. The protocol bounds rounds to 32; this cap is kept locally too.
    pub fn record_model_activity_for(&mut self, run_id: &str, round: ModelActivityRound) {
        if self.active_run.as_deref() != Some(run_id) {
            return;
        }
        if let Some(existing) = self
            .model_activity
            .iter_mut()
            .find(|existing| existing.round == round.round)
        {
            *existing = round;
        } else {
            self.model_activity.push(round);
            self.model_activity.sort_by_key(|entry| entry.round);
            if self.model_activity.len() > 32 {
                self.model_activity.remove(0);
            }
        }
    }

    pub fn toggle_compact(&mut self) {
        self.mode = match self.mode {
            WindowMode::Compact => WindowMode::Conversation,
            _ => WindowMode::Compact,
        };
    }

    /// Keep the only Vox window reachable. Until the desktop integration has
    /// a verified way to reactivate a hidden window (such as a tray item or
    /// global invocation), the visible control always enters the compact
    /// accompaniment instead of minimizing the window.
    pub fn request_compact_accompaniment(&mut self) {
        self.mode = WindowMode::Compact;
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
    fn compact_accompaniment_keeps_the_only_window_reachable_for_idle_and_active_runs() {
        let mut active = PresentationState::default();
        active.submit("faça", "run");

        active.request_compact_accompaniment();
        assert_eq!(active.mode, WindowMode::Compact);
        assert_eq!(active.active_run.as_deref(), Some("run"));

        let mut idle = PresentationState::default();
        idle.request_compact_accompaniment();
        assert_eq!(idle.mode, WindowMode::Compact);
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

    #[test]
    fn failed_dispatch_withdraws_only_the_optimistic_bubble_and_keeps_the_draft() {
        let mut state = PresentationState {
            draft: "tente novamente".into(),
            ..Default::default()
        };
        state.submit("tente novamente", "run");

        state.withdraw_submission_for("run");

        assert!(state.messages.is_empty());
        assert_eq!(state.draft, "tente novamente");
        assert_eq!(state.active_run, None);
    }

    #[test]
    fn model_activity_is_bound_to_the_active_run_and_survives_completion() {
        let mut state = PresentationState::default();
        state.submit("oi", "run-current");
        state.record_model_activity_for(
            "run-old",
            ModelActivityRound {
                round: 1,
                provider: "fake".into(),
                model_ref: None,
                messages: vec![],
            },
        );
        assert!(state.model_activity.is_empty());
        state.record_model_activity_for(
            "run-current",
            ModelActivityRound {
                round: 1,
                provider: "fake".into(),
                model_ref: Some("fake-text".into()),
                messages: vec![ModelActivityMessage {
                    role: "user".into(),
                    content: "pedido redigido".into(),
                }],
            },
        );
        state.complete_for("run-current", "feito");
        assert_eq!(state.model_activity.len(), 1);
        assert!(!state.show_activity);
    }
}

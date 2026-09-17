use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    None,
    Read,
    Write,
    External,
    Arbitrary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    RequireApproval,
}

#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub name: String,
    pub effect: EffectClass,
    pub auto_approve: bool,
    pub max_argument_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct ApprovalToken {
    pub approval_id: Uuid,
    pub tool: String,
    pub args_hash: String,
    pub scope: String,
    pub expires_at: SystemTime,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("tool is not registered: {0}")]
    UnknownTool(String),
    #[error("arguments exceed the tool limit")]
    ArgumentsTooLarge,
    #[error("approval is required")]
    ApprovalRequired,
    #[error("approval is invalid, expired, or scoped to another call")]
    InvalidApproval,
    #[error("effect is outside the current scope")]
    OutOfScope,
}

pub struct PolicyEngine {
    tools: BTreeMap<String, ToolDescriptor>,
    approvals: Arc<Mutex<BTreeMap<Uuid, ApprovalToken>>>,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        let descriptors = [
            ("desktop.list_windows", EffectClass::Read, true),
            ("desktop.snapshot", EffectClass::Read, true),
            ("desktop.query", EffectClass::Read, true),
            ("desktop.wait_for", EffectClass::Read, true),
            ("clipboard.read", EffectClass::Read, true),
            ("clipboard.write", EffectClass::Write, false),
            ("files.search", EffectClass::Read, true),
            ("files.list", EffectClass::Read, true),
            ("files.read", EffectClass::Read, true),
            ("files.write", EffectClass::Write, false),
            ("files.move", EffectClass::Write, false),
            ("shell.exec", EffectClass::Arbitrary, false),
            ("process.terminate", EffectClass::External, false),
            ("process.list", EffectClass::Read, true),
            ("apps.resolve", EffectClass::Read, true),
            ("apps.launch", EffectClass::External, false),
            ("paths.open", EffectClass::External, false),
            ("desktop.act", EffectClass::Write, false),
        ]
        .into_iter()
        .map(|(name, effect, auto_approve)| {
            (
                name.to_string(),
                ToolDescriptor {
                    name: name.to_string(),
                    effect,
                    auto_approve,
                    max_argument_bytes: 64 * 1024,
                },
            )
        })
        .collect();
        Self {
            tools: descriptors,
            approvals: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

impl PolicyEngine {
    pub fn descriptor(&self, tool: &str) -> Result<&ToolDescriptor, PolicyError> {
        self.tools
            .get(tool)
            .ok_or_else(|| PolicyError::UnknownTool(tool.to_string()))
    }

    pub fn arguments_hash(&self, args: &serde_json::Value) -> String {
        let canonical = serde_json::to_vec(args).unwrap_or_default();
        let digest = Sha256::digest(canonical);
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn decide(&self, tool: &str, args: &serde_json::Value) -> Result<Decision, PolicyError> {
        let descriptor = self.descriptor(tool)?;
        let size = serde_json::to_vec(args)
            .map_err(|_| PolicyError::ArgumentsTooLarge)?
            .len();
        if size > descriptor.max_argument_bytes {
            return Err(PolicyError::ArgumentsTooLarge);
        }
        Ok(if descriptor.auto_approve {
            Decision::Allow
        } else {
            Decision::RequireApproval
        })
    }

    pub fn issue_approval(
        &self,
        tool: &str,
        args: &serde_json::Value,
        scope: impl Into<String>,
        ttl: Duration,
    ) -> Result<ApprovalToken, PolicyError> {
        let descriptor = self.descriptor(tool)?;
        if descriptor.effect == EffectClass::None || descriptor.auto_approve {
            return Err(PolicyError::OutOfScope);
        }
        let scope = scope.into();
        if scope.is_empty() {
            return Err(PolicyError::OutOfScope);
        }
        let token = ApprovalToken {
            approval_id: Uuid::new_v4(),
            tool: tool.to_string(),
            args_hash: self.arguments_hash(args),
            scope,
            expires_at: SystemTime::now() + ttl,
        };
        self.approvals
            .lock()
            .map_err(|_| PolicyError::InvalidApproval)?
            .insert(token.approval_id, token.clone());
        Ok(token)
    }

    pub fn authorize(
        &self,
        tool: &str,
        args: &serde_json::Value,
        approval: Option<&ApprovalToken>,
    ) -> Result<Decision, PolicyError> {
        let decision = self.decide(tool, args)?;
        if decision == Decision::Allow {
            return Ok(Decision::Allow);
        }
        let approval = approval.ok_or(PolicyError::ApprovalRequired)?;
        let mut approvals = self
            .approvals
            .lock()
            .map_err(|_| PolicyError::InvalidApproval)?;
        let stored = approvals
            .get(&approval.approval_id)
            .ok_or(PolicyError::InvalidApproval)?;
        if stored.tool != tool
            || stored.args_hash != self.arguments_hash(args)
            || stored.expires_at <= SystemTime::now()
            || approval.expires_at <= SystemTime::now()
            || stored.scope.is_empty()
            || stored.tool != approval.tool
            || stored.args_hash != approval.args_hash
        {
            return Err(PolicyError::InvalidApproval);
        }
        approvals.remove(&approval.approval_id);
        Ok(Decision::Allow)
    }

    /// Authorize a broker call using the opaque approval id sent over IPC.
    /// The id is looked up inside the policy engine and consumed atomically on
    /// success, so replaying the same approval cannot repeat an effect.
    pub fn authorize_ref(
        &self,
        tool: &str,
        args: &serde_json::Value,
        scope: &str,
        approval_id: Option<&str>,
    ) -> Result<Decision, PolicyError> {
        let decision = self.decide(tool, args)?;
        if decision == Decision::Allow {
            return Ok(Decision::Allow);
        }
        let raw = approval_id.ok_or(PolicyError::ApprovalRequired)?;
        let id = Uuid::parse_str(raw).map_err(|_| PolicyError::InvalidApproval)?;
        let token = self
            .approvals
            .lock()
            .map_err(|_| PolicyError::InvalidApproval)?
            .get(&id)
            .cloned()
            .ok_or(PolicyError::InvalidApproval)?;
        // An approval is intentionally scoped outside of the model-visible
        // arguments.  In particular, an identical write in another run must
        // never be able to consume an approval shown for this run.
        if token.scope != scope {
            return Err(PolicyError::InvalidApproval);
        }
        self.authorize(tool, args, Some(&token))
    }

    pub fn revoke_approval(&self, approval_id: &str) -> Result<(), PolicyError> {
        let id = Uuid::parse_str(approval_id).map_err(|_| PolicyError::InvalidApproval)?;
        self.approvals
            .lock()
            .map_err(|_| PolicyError::InvalidApproval)?
            .remove(&id)
            .map(|_| ())
            .ok_or(PolicyError::InvalidApproval)
    }

    pub fn approval_expiry_epoch(token: &ApprovalToken) -> u64 {
        token
            .expires_at
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_tools_are_auto_approved() {
        let policy = PolicyEngine::default();
        assert_eq!(
            policy
                .authorize("desktop.list_windows", &serde_json::json!({}), None)
                .unwrap(),
            Decision::Allow
        );
    }

    #[test]
    fn a02_approval_must_match_the_exact_call_shape() {
        let policy = PolicyEngine::default();
        let args = serde_json::json!({"path":"/tmp/a","content":"safe"});
        assert_eq!(
            policy.authorize("files.write", &args, None),
            Err(PolicyError::ApprovalRequired)
        );
        let approval = policy
            .issue_approval("files.write", &args, "path:/tmp/a", Duration::from_secs(60))
            .unwrap();
        assert_eq!(
            policy
                .authorize("files.write", &args, Some(&approval))
                .unwrap(),
            Decision::Allow
        );
        assert_eq!(
            policy.authorize(
                "files.write",
                &serde_json::json!({"path":"/tmp/b","content":"safe"}),
                Some(&approval)
            ),
            Err(PolicyError::InvalidApproval)
        );
    }

    #[test]
    fn expired_approval_is_rejected() {
        let policy = PolicyEngine::default();
        let args = serde_json::json!({"command":"true"});
        let mut approval = policy
            .issue_approval("shell.exec", &args, "shell", Duration::from_secs(1))
            .unwrap();
        approval.expires_at = SystemTime::now() - Duration::from_secs(1);
        assert_eq!(
            policy.authorize("shell.exec", &args, Some(&approval)),
            Err(PolicyError::InvalidApproval)
        );
    }

    #[test]
    fn a03_replaying_an_approval_is_rejected() {
        let policy = PolicyEngine::default();
        let args = serde_json::json!({"path":"/tmp/a"});
        let approval = policy
            .issue_approval("files.write", &args, "run:r", Duration::from_secs(60))
            .unwrap();
        let id = approval.approval_id.to_string();
        assert_eq!(
            policy
                .authorize_ref("files.write", &args, "run:r", Some(&id))
                .unwrap(),
            Decision::Allow
        );
        assert_eq!(
            policy.authorize_ref("files.write", &args, "run:r", Some(&id)),
            Err(PolicyError::InvalidApproval)
        );
    }

    #[test]
    fn approval_cannot_be_consumed_by_another_run() {
        let policy = PolicyEngine::default();
        let args = serde_json::json!({"path":"/tmp/a"});
        let approval = policy
            .issue_approval(
                "files.write",
                &args,
                "run:original",
                Duration::from_secs(60),
            )
            .unwrap();
        let id = approval.approval_id.to_string();

        assert_eq!(
            policy.authorize_ref("files.write", &args, "run:other", Some(&id)),
            Err(PolicyError::InvalidApproval)
        );
        // The rejected cross-run attempt must not consume the original
        // approval.  The user can still approve the exact action they saw.
        assert_eq!(
            policy
                .authorize_ref("files.write", &args, "run:original", Some(&id))
                .unwrap(),
            Decision::Allow
        );
    }

    #[test]
    fn a01_unknown_and_oversized_calls_are_rejected_before_effects() {
        let policy = PolicyEngine::default();
        assert_eq!(
            policy.decide("unknown.tool", &serde_json::json!({})),
            Err(PolicyError::UnknownTool("unknown.tool".into()))
        );
        let oversized = serde_json::json!({"content": "x".repeat(64 * 1024)});
        assert_eq!(
            policy.decide("files.write", &oversized),
            Err(PolicyError::ArgumentsTooLarge)
        );
    }
}

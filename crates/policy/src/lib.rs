use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    None,
    Read,
    Write,
    External,
    Arbitrary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub name: String,
    pub effect: EffectClass,
    pub max_argument_bytes: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("tool is not registered: {0}")]
    UnknownTool(String),
    #[error("arguments exceed the tool limit")]
    ArgumentsTooLarge,
    #[error("effect is outside the active task grant")]
    OutOfScope,
}

/// Structural policy is independent from model confidence. A task grant
/// authorizes a run; this policy still enforces the registered vocabulary and
/// bounded arguments before the runtime sees a call.
pub struct PolicyEngine {
    tools: BTreeMap<String, ToolDescriptor>,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        let descriptors = [
            ("desktop.list_windows", EffectClass::Read),
            ("desktop.snapshot", EffectClass::Read),
            ("desktop.query", EffectClass::Read),
            ("desktop.wait_for", EffectClass::Read),
            ("clipboard.read", EffectClass::Read),
            ("clipboard.write", EffectClass::Write),
            ("files.search", EffectClass::Read),
            ("files.list", EffectClass::Read),
            ("files.read", EffectClass::Read),
            ("files.write", EffectClass::Write),
            ("files.move", EffectClass::Write),
            ("shell.exec", EffectClass::Arbitrary),
            ("process.terminate", EffectClass::External),
            ("process.list", EffectClass::Read),
            ("apps.resolve", EffectClass::Read),
            ("apps.launch", EffectClass::External),
            ("paths.open", EffectClass::External),
            ("desktop.act", EffectClass::Write),
        ]
        .into_iter()
        .map(|(name, effect)| {
            (
                name.to_owned(),
                ToolDescriptor {
                    name: name.to_owned(),
                    effect,
                    max_argument_bytes: 64 * 1024,
                },
            )
        })
        .collect();
        Self { tools: descriptors }
    }
}

impl PolicyEngine {
    pub fn descriptor(&self, tool: &str) -> Result<&ToolDescriptor, PolicyError> {
        self.tools
            .get(tool)
            .ok_or_else(|| PolicyError::UnknownTool(tool.to_owned()))
    }

    pub fn validate(
        &self,
        tool: &str,
        args: &serde_json::Value,
    ) -> Result<&ToolDescriptor, PolicyError> {
        let descriptor = self.descriptor(tool)?;
        if serde_json::to_vec(args)
            .map_err(|_| PolicyError::ArgumentsTooLarge)?
            .len()
            > descriptor.max_argument_bytes
        {
            return Err(PolicyError::ArgumentsTooLarge);
        }
        Ok(descriptor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_calls_are_bounded_by_the_tool_policy() {
        let policy = PolicyEngine::default();
        assert_eq!(
            policy
                .validate("shell.exec", &serde_json::json!({"program":"true"}))
                .unwrap()
                .effect,
            EffectClass::Arbitrary
        );
        assert_eq!(
            policy.validate("unknown.tool", &serde_json::json!({})),
            Err(PolicyError::UnknownTool("unknown.tool".into()))
        );
    }
}

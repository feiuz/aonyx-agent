//! Approval gate.
//!
//! Sits between the runner and any destructive tool call. The default V1
//! policy is conservative — every [`SafetyClass::Destructive`] call is
//! rejected until an explicit policy says otherwise. Interactive runs swap in
//! a policy that prompts the user; non-interactive runs lean on
//! [`ApprovalPolicy::DenyDestructive`].

use std::sync::Arc;

use aonyx_core::{SafetyClass, ToolCall};

/// Approval policy for tool calls.
#[derive(Clone)]
pub enum ApprovalPolicy {
    /// Allow every call regardless of class. Intended for trusted contexts
    /// (e.g. CI runs against a sandboxed workdir, or unit tests).
    AutoAllow,
    /// Allow [`SafetyClass::Safe`] and [`SafetyClass::Caution`], reject every
    /// destructive call. This is the V1 default for non-interactive runs.
    DenyDestructive,
    /// Defer to a custom predicate — used by interactive CLI prompts.
    Custom(Arc<dyn Fn(&ToolCall, SafetyClass) -> bool + Send + Sync>),
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self::DenyDestructive
    }
}

impl ApprovalPolicy {
    /// Build a custom policy from any thread-safe predicate.
    pub fn custom<F>(f: F) -> Self
    where
        F: Fn(&ToolCall, SafetyClass) -> bool + Send + Sync + 'static,
    {
        Self::Custom(Arc::new(f))
    }

    /// Returns `true` when the call may proceed.
    pub fn allow(&self, call: &ToolCall, class: SafetyClass) -> bool {
        match self {
            Self::AutoAllow => true,
            Self::DenyDestructive => class != SafetyClass::Destructive,
            Self::Custom(f) => f(call, class),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use uuid::Uuid;

    fn dummy_call() -> ToolCall {
        ToolCall {
            id: Uuid::new_v4().to_string(),
            name: "fs_write".to_string(),
            args: Value::Null,
        }
    }

    #[test]
    fn auto_allow_accepts_destructive() {
        assert!(ApprovalPolicy::AutoAllow.allow(&dummy_call(), SafetyClass::Destructive));
    }

    #[test]
    fn deny_destructive_default_rejects_destructive() {
        let p = ApprovalPolicy::default();
        assert!(!p.allow(&dummy_call(), SafetyClass::Destructive));
        assert!(p.allow(&dummy_call(), SafetyClass::Caution));
        assert!(p.allow(&dummy_call(), SafetyClass::Safe));
    }

    #[test]
    fn custom_policy_runs_predicate() {
        let p = ApprovalPolicy::custom(|call, class| {
            class == SafetyClass::Destructive && call.name == "fs_write"
        });
        assert!(p.allow(&dummy_call(), SafetyClass::Destructive));
        assert!(!p.allow(&dummy_call(), SafetyClass::Safe));
    }
}

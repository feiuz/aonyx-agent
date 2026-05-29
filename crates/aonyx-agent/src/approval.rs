//! Approval gate.
//!
//! Sits between the runner and any destructive tool call. The default V1
//! policy is conservative — every [`SafetyClass::Destructive`] call is
//! rejected until an explicit policy says otherwise. Interactive runs swap in
//! a policy that prompts the user; non-interactive runs lean on
//! [`ApprovalPolicy::DenyDestructive`].

use std::sync::Arc;

use aonyx_core::{SafetyClass, ToolCall};
use async_trait::async_trait;

/// Predicate signature used by [`ApprovalPolicy::Custom`].
pub type ApprovalPredicate = Arc<dyn Fn(&ToolCall, SafetyClass) -> bool + Send + Sync>;

/// Async approver — typically a UI bridge that pauses the runner while
/// it asks the user (Phase P).
#[async_trait]
pub trait AsyncApprover: Send + Sync + std::fmt::Debug {
    /// Return `true` to let `call` proceed, `false` to reject it.
    async fn approve(&self, call: &ToolCall, class: SafetyClass) -> bool;
}

/// Approval policy for tool calls.
#[derive(Clone, Default)]
pub enum ApprovalPolicy {
    /// Allow every call regardless of class. Intended for trusted contexts
    /// (e.g. CI runs against a sandboxed workdir, or unit tests).
    AutoAllow,
    /// Allow [`SafetyClass::Safe`] and [`SafetyClass::Caution`], reject every
    /// destructive call. This is the V1 default for non-interactive runs.
    #[default]
    DenyDestructive,
    /// Defer to a custom predicate — used by interactive CLI prompts.
    Custom(ApprovalPredicate),
    /// Defer to an async approver — used by the full-screen TUI to
    /// pause the runner while a `[Y/n]` overlay collects the user's
    /// decision (Phase P).
    Interactive(Arc<dyn AsyncApprover>),
}

impl std::fmt::Debug for ApprovalPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AutoAllow => write!(f, "AutoAllow"),
            Self::DenyDestructive => write!(f, "DenyDestructive"),
            Self::Custom(_) => write!(f, "Custom(<fn>)"),
            Self::Interactive(a) => write!(f, "Interactive({a:?})"),
        }
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

    /// Wrap an [`AsyncApprover`] in an [`ApprovalPolicy::Interactive`].
    pub fn interactive(a: Arc<dyn AsyncApprover>) -> Self {
        Self::Interactive(a)
    }

    /// Returns `true` when the call may proceed. Async because the
    /// `Interactive` variant may need to await user input.
    pub async fn allow(&self, call: &ToolCall, class: SafetyClass) -> bool {
        match self {
            Self::AutoAllow => true,
            Self::DenyDestructive => class != SafetyClass::Destructive,
            Self::Custom(f) => f(call, class),
            Self::Interactive(a) => a.approve(call, class).await,
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

    #[tokio::test]
    async fn auto_allow_accepts_destructive() {
        assert!(
            ApprovalPolicy::AutoAllow
                .allow(&dummy_call(), SafetyClass::Destructive)
                .await
        );
    }

    #[tokio::test]
    async fn deny_destructive_default_rejects_destructive() {
        let p = ApprovalPolicy::default();
        assert!(!p.allow(&dummy_call(), SafetyClass::Destructive).await);
        assert!(p.allow(&dummy_call(), SafetyClass::Caution).await);
        assert!(p.allow(&dummy_call(), SafetyClass::Safe).await);
    }

    #[tokio::test]
    async fn custom_policy_runs_predicate() {
        let p = ApprovalPolicy::custom(|call, class| {
            class == SafetyClass::Destructive && call.name == "fs_write"
        });
        assert!(p.allow(&dummy_call(), SafetyClass::Destructive).await);
        assert!(!p.allow(&dummy_call(), SafetyClass::Safe).await);
    }

    #[derive(Debug)]
    struct AlwaysApprove;

    #[async_trait]
    impl AsyncApprover for AlwaysApprove {
        async fn approve(&self, _call: &ToolCall, _class: SafetyClass) -> bool {
            true
        }
    }

    #[derive(Debug)]
    struct AlwaysDeny;

    #[async_trait]
    impl AsyncApprover for AlwaysDeny {
        async fn approve(&self, _call: &ToolCall, _class: SafetyClass) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn interactive_delegates_to_approver() {
        let p = ApprovalPolicy::interactive(Arc::new(AlwaysApprove));
        assert!(p.allow(&dummy_call(), SafetyClass::Destructive).await);
        let p = ApprovalPolicy::interactive(Arc::new(AlwaysDeny));
        assert!(!p.allow(&dummy_call(), SafetyClass::Destructive).await);
    }
}

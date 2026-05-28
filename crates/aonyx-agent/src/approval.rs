//! Approval gate.
//!
//! Intercepts every tool call classified as [`SafetyClass::Destructive`]
//! ([`aonyx_core::SafetyClass`]) and runs it through a configurable policy:
//! - Interactive CLI: prompt the user (`[y/N]`).
//! - Non-interactive: read the policy file (`~/.aonyx/approvals.toml`) to
//!   decide allow / deny / always-ask.
//! - On reject: return [`aonyx_core::AonyxError::ApprovalRejected`].

// TODO(V1): policy file format + CLI prompt integration.

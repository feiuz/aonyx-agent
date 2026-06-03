//! Shared application state and its value types.
//!
//! [`ApiState`] is cloned into every handler by axum, so its fields are
//! cheap-to-clone `Arc`s. The session store and the turn-runner are trait
//! objects so the binary injects its real implementations while tests use
//! an in-memory store + a stub agent.

use std::sync::Arc;

use aonyx_memory::SessionStore;
use serde::Serialize;

use crate::agent::ApiAgent;

/// Authentication + authorization policy for the API.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Bearer token required on protected routes. `None` disables auth
    /// (only safe on a loopback bind — the binary enforces that in V4.5).
    pub token: Option<String>,
    /// Whether [`aonyx_core::SafetyClass::Destructive`] tools may be invoked
    /// through the direct tool endpoint. Defaults to `false`.
    pub allow_destructive: bool,
}

impl AuthConfig {
    /// Build a policy from an optional token and the destructive-tool flag.
    pub fn new(token: Option<String>, allow_destructive: bool) -> Self {
        Self {
            token,
            allow_destructive,
        }
    }

    /// Returns `true` when the request is authorized: either no token is
    /// configured, or `auth_header` carries the matching
    /// `Authorization: Bearer <token>` (the `Bearer ` prefix is optional).
    pub fn check(&self, auth_header: Option<&str>) -> bool {
        match &self.token {
            None => true,
            Some(expected) => auth_header
                .map(|h| h.strip_prefix("Bearer ").unwrap_or(h) == expected)
                .unwrap_or(false),
        }
    }
}

/// Server identity + capabilities, returned by `GET /v1/info`.
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    /// Product name (`"aonyx-agent"`).
    pub name: &'static str,
    /// Crate version (`CARGO_PKG_VERSION`).
    pub version: &'static str,
    /// Active LLM provider id (e.g. `"anthropic"`).
    pub provider: String,
    /// Active default model id.
    pub model: String,
    /// Enabled capability flags (e.g. `"streaming"`, `"tools"`).
    pub features: Vec<String>,
}

impl ServerInfo {
    /// Build server info for the active provider/model and capability set.
    pub fn new(
        provider: impl Into<String>,
        model: impl Into<String>,
        features: Vec<String>,
    ) -> Self {
        Self {
            name: "aonyx-agent",
            version: env!("CARGO_PKG_VERSION"),
            provider: provider.into(),
            model: model.into(),
            features,
        }
    }
}

/// State shared with every request handler.
#[derive(Clone)]
pub struct ApiState {
    /// Auth + authorization policy.
    pub auth: Arc<AuthConfig>,
    /// Static server/capability info for `GET /v1/info`.
    pub info: Arc<ServerInfo>,
    /// Persistent session store (typically `~/.aonyx/sessions.db`).
    pub sessions: Arc<dyn SessionStore>,
    /// The injected agent loop used to run a turn.
    pub agent: Arc<dyn ApiAgent>,
    /// Project slug used when a request does not specify one.
    pub default_project: Arc<String>,
}

impl ApiState {
    /// Assemble the state from its parts.
    pub fn new(
        auth: AuthConfig,
        info: ServerInfo,
        sessions: Arc<dyn SessionStore>,
        agent: Arc<dyn ApiAgent>,
        default_project: impl Into<String>,
    ) -> Self {
        Self {
            auth: Arc::new(auth),
            info: Arc::new(info),
            sessions,
            agent,
            default_project: Arc::new(default_project.into()),
        }
    }

    /// The given project, or the server default when `None`/empty.
    pub(crate) fn project_or_default(&self, project: Option<String>) -> String {
        project
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.default_project.as_ref().clone())
    }
}

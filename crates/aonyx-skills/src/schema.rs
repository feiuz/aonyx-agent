//! Parsed SKILL.md representation.

use serde::{Deserialize, Serialize};

/// Trigger configuration controlling when a skill becomes active.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trigger {
    /// Case-insensitive substring matches.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Regex patterns applied to the user query.
    #[serde(default)]
    pub query_matches: Vec<String>,
    /// Regex pattern applied to the active project slug.
    #[serde(default)]
    pub project_matches: Option<String>,
    /// Activated only via explicit user opt-in.
    #[serde(default)]
    pub manual: bool,
    /// Always active when enabled.
    #[serde(default)]
    pub always_on: bool,
}

/// A skill loaded from a `SKILL.md` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Stable identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Globally enabled in user config.
    #[serde(default)]
    pub enabled: bool,
    /// Tool whitelist; empty = inherit registry default.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Activation triggers.
    #[serde(default)]
    pub trigger: Trigger,
    /// Markdown body injected as a system prompt fragment.
    #[serde(default)]
    pub body: String,
}

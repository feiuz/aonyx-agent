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
///
/// The schema is a **superset** of the open agent-skills standard (Hermes,
/// Claude Code, Cursor): the Aonyx-native fields (`id`, `trigger`, `tools`)
/// sit alongside the portable ones (`description`, `version`, `author`,
/// `license`, `platforms`, `metadata`). Unknown frontmatter keys are ignored,
/// so a Hermes `SKILL.md` parses unchanged; the loader fills `id` and derives
/// trigger keywords when they're absent. `category`/`tags` feed the catalogue UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Stable identifier. Derived from `name` when omitted (Hermes has no `id`).
    #[serde(default)]
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// One-line summary (the portable `description` field).
    #[serde(default)]
    pub description: Option<String>,
    /// Catalogue category — derived from the directory when omitted.
    #[serde(default)]
    pub category: Option<String>,
    /// Free-form tags (a flat list, or lifted from `metadata.hermes.tags`).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Skill version (portable).
    #[serde(default)]
    pub version: Option<String>,
    /// Author attribution (portable).
    #[serde(default)]
    pub author: Option<String>,
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

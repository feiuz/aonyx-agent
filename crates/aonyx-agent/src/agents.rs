//! Custom sub-agents (ADR-017).
//!
//! Definitions are loaded from `~/.aonyx/agents/*.AGENT.md` plus built-in
//! presets, and offered to the **architect** (the main chat agent) which
//! delegates to them via the `dispatch_agent` tool. Each sub-agent runs
//! isolated — its own tool whitelist + model — but shares the project's memory
//! palace (the memory tools carry a `Palace` clone), so delegation never loses
//! context. Mirrors the `SKILL.md` format/loader in `aonyx-skills`.

use serde::{Deserialize, Serialize};
use std::path::Path;

fn default_true() -> bool {
    true
}

/// A sub-agent definition — a Skill-like file plus model/provider for isolation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Stable id (frontmatter, else derived from the name / file name).
    #[serde(default)]
    pub id: String,
    /// Human name.
    pub name: String,
    /// When to use this agent — shown to the architect so it can route a task.
    #[serde(default)]
    pub description: String,
    /// Model override; `None` inherits the architect's model.
    #[serde(default)]
    pub model: Option<String>,
    /// Provider override; `None` inherits the architect's provider.
    #[serde(default)]
    pub provider: Option<String>,
    /// Tool whitelist; empty = inherit the parent registry.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Disabled agents are not offered to the architect.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Per-agent tool-call iteration cap; `None` uses the runner default.
    #[serde(default)]
    pub max_iterations: Option<usize>,
    /// Markdown body = the sub-agent's system prompt.
    #[serde(default)]
    pub body: String,
}

impl AgentDefinition {
    /// Parse an `AGENT.md` (YAML frontmatter + markdown body).
    pub fn parse(raw: &str) -> std::result::Result<Self, String> {
        let (fm, body) = split_frontmatter(raw).ok_or("missing or malformed YAML frontmatter")?;
        let mut def: AgentDefinition =
            serde_yaml::from_str(fm).map_err(|e| format!("parse frontmatter: {e}"))?;
        def.body = body.to_string();
        if def.id.trim().is_empty() {
            def.id = slug(&def.name);
        }
        Ok(def)
    }
}

/// All agents available to the architect: built-in presets overlaid with the
/// user's `~/.aonyx/agents/` (user wins on id collision); disabled ones dropped.
pub fn load_all(agents_dir: impl AsRef<Path>) -> Vec<AgentDefinition> {
    let mut by_id: std::collections::BTreeMap<String, AgentDefinition> =
        builtin().into_iter().map(|a| (a.id.clone(), a)).collect();
    for a in load_dir(agents_dir) {
        by_id.insert(a.id.clone(), a);
    }
    by_id.into_values().filter(|a| a.enabled).collect()
}

/// Parse every `AGENT.md` / `*.agent.md` directly under `dir` (non-recursive;
/// malformed files are skipped with a warning).
pub fn load_dir(dir: impl AsRef<Path>) -> Vec<AgentDefinition> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir.as_ref()) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let lower = fname.to_ascii_lowercase();
        if lower != "agent.md" && !lower.ends_with(".agent.md") {
            continue;
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|raw| AgentDefinition::parse(&raw))
        {
            Ok(mut def) => {
                if def.id.trim().is_empty() {
                    def.id = slug(lower.trim_end_matches(".agent.md").trim_end_matches("agent.md"));
                }
                out.push(def);
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "skipping malformed agent");
            }
        }
    }
    out
}

/// Built-in preset sub-agents (embedded at compile time).
pub fn builtin() -> Vec<AgentDefinition> {
    [CODER, REVIEWER, RESEARCHER]
        .iter()
        .filter_map(|raw| AgentDefinition::parse(raw).ok())
        .collect()
}

fn slug(s: &str) -> String {
    let raw: String = s
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    raw.trim_matches('-').to_string()
}

/// Split `---\n…\n---\n<body>` into `(frontmatter, body)`. Mirrors the skills loader.
fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let after_start = raw
        .strip_prefix("---\r\n")
        .or_else(|| raw.strip_prefix("---\n"))?;
    let mut cursor = 0;
    while let Some(rel) = after_start[cursor..].find("\n---") {
        let abs = cursor + rel;
        let after = &after_start[abs + 4..];
        if after.is_empty() || after.starts_with('\n') || after.starts_with('\r') {
            let frontmatter = &after_start[..abs];
            let body = after.trim_start_matches(['\r', '\n']);
            return Some((frontmatter, body));
        }
        cursor = abs + 4;
    }
    None
}

const CODER: &str = r#"---
id: coder
name: Coder
description: Write or modify code — implement features, fix bugs, refactor, inspect the build.
tools: [fs_read, fs_write, fs_edit, fs_glob, fs_grep, bash, git_status, git_diff]
---
You are a senior software engineer. Write idiomatic code that matches the
surrounding repository's conventions. Read before you edit, make the smallest
change that solves the task, verify it (build/tests when available), and report
the files you touched. Be precise and concise.
"#;

const REVIEWER: &str = r#"---
id: reviewer
name: Reviewer
description: Review code or a diff for bugs, risks, and quality. Read-only — never edits files.
tools: [fs_read, fs_glob, fs_grep, git_diff, git_show]
---
You are a meticulous code reviewer. Inspect the code or diff, find real bugs,
security issues, and design problems, and explain each with a file:line
reference and a concrete fix. Be specific; skip nitpicks unless asked. You never
modify files.
"#;

const RESEARCHER: &str = r#"---
id: researcher
name: Researcher
description: Gather context — search the project memory palace, and the web when permitted.
tools: [rag_search, memory_search, web_search, web_fetch]
---
You research a question and return a sourced synthesis. Prefer the project's
memory palace (rag_search / memory_search) first, then the web when permitted.
Cite every claim with its source. Be thorough but concise.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_presets_parse() {
        let agents = builtin();
        let mut ids: Vec<&str> = agents.iter().map(|a| a.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["coder", "researcher", "reviewer"]);
        assert!(agents.iter().all(|a| !a.body.trim().is_empty()));
        assert!(agents.iter().all(|a| !a.description.is_empty()));
    }

    #[test]
    fn parse_minimal_agent() {
        let raw = "---\nname: Tester\ndescription: runs tests\ntools: [bash]\n---\nYou run the test suite.\n";
        let a = AgentDefinition::parse(raw).unwrap();
        assert_eq!(a.id, "tester");
        assert_eq!(a.name, "Tester");
        assert_eq!(a.tools, vec!["bash"]);
        assert!(a.enabled);
        assert!(a.body.contains("test suite"));
    }
}

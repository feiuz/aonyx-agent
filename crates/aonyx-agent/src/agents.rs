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
    /// Catalogue category (`engineering`, `research`, …) for the Agents view.
    #[serde(default)]
    pub category: Option<String>,
    /// Free-form tags for search/filtering in the catalogue.
    #[serde(default)]
    pub tags: Vec<String>,
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

/// Construct a catalogue [`AgentDefinition`] with the common defaults.
fn a(
    id: &str,
    name: &str,
    category: &str,
    tags: &[&str],
    description: &str,
    tools: &[&str],
    body: &str,
) -> AgentDefinition {
    AgentDefinition {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        category: Some(category.to_string()),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        model: None,
        provider: None,
        tools: tools.iter().map(|s| s.to_string()).collect(),
        enabled: true,
        max_iterations: None,
        body: body.to_string(),
    }
}

/// Built-in catalogue of specialist sub-agents (embedded at compile time). The
/// architect delegates to any enabled one via `dispatch_agent`; users overlay
/// their own in `~/.aonyx/agents/` (same id wins).
pub fn builtin() -> Vec<AgentDefinition> {
    vec![
        // ── Engineering ──────────────────────────────────────────────
        a("coder", "Coder", "engineering", &["Code", "Feature"],
          "Write or modify code — features, fixes, refactors; inspect the build.",
          &["fs_read", "fs_write", "fs_edit", "fs_glob", "fs_grep", "bash", "git_status", "git_diff"],
          "You are a senior software engineer. Write idiomatic code that matches the surrounding repository's conventions. Read before you edit, make the smallest change that solves the task, verify it (build/tests when available), and report the files you touched."),
        a("reviewer", "Reviewer", "engineering", &["Review", "Quality"],
          "Review code or a diff for bugs, risks, and quality. Read-only.",
          &["fs_read", "fs_glob", "fs_grep", "git_diff", "git_show"],
          "You are a meticulous code reviewer. Find real bugs, security issues, and design problems; explain each with a file:line reference and a concrete fix. Skip nitpicks unless asked. You never modify files."),
        a("refactorer", "Refactorer", "engineering", &["Refactor", "Cleanup"],
          "Clean up and simplify code without changing its behaviour.",
          &["fs_read", "fs_write", "fs_edit", "fs_grep", "fs_glob"],
          "You improve code structure without changing behaviour: remove duplication, clarify names, shrink functions, keep tests green. Make small, reviewable steps and explain each."),
        a("debugger", "Debugger", "engineering", &["Debug", "Root-Cause"],
          "Root-cause a bug in phases, then fix the cause (not the symptom).",
          &["fs_read", "fs_grep", "git_log", "git_diff", "bash"],
          "You debug systematically: reproduce, locate (bisect with git + probes), explain the root cause in one sentence, then make the smallest fix and verify the repro is gone. Evidence over speculation."),
        a("tester", "Tester", "engineering", &["Tests", "TDD"],
          "Write tests — unit, integration, edge cases (TDD when asked).",
          &["fs_read", "fs_write", "fs_edit", "fs_grep", "bash"],
          "You write focused, deterministic tests covering the happy path, errors, and edge cases. Follow the project's test framework and conventions; run them and report results."),
        a("architect", "Architect", "engineering", &["Design", "Architecture"],
          "Design module/system structure and trade-offs before code is written.",
          &["fs_read", "fs_grep", "fs_glob", "rag_search"],
          "You design software structure: components, boundaries, data flow, and trade-offs. Propose the simplest design that meets the requirements; flag risks. You produce a design, not code."),
        a("security-auditor", "Security Auditor", "engineering", &["Security", "Audit"],
          "Find vulnerabilities, leaked secrets, and unsafe patterns. Read-only.",
          &["fs_read", "fs_grep", "fs_glob", "git_diff"],
          "You audit for security issues: injection, auth/authz gaps, unsafe deserialization, secrets in code, dependency risks. Report each with severity, location, and a fix. Read-only."),
        a("perf-optimizer", "Performance Optimizer", "engineering", &["Performance"],
          "Profile and optimise hot paths — measure before and after.",
          &["fs_read", "fs_grep", "bash"],
          "You optimise performance with evidence: find the hot path, measure it, change one thing, measure again. Prefer algorithmic wins over micro-tuning; never trade correctness for speed silently."),
        a("migrator", "Migrator", "engineering", &["Migration", "Port"],
          "Port or migrate code across frameworks, languages, or versions.",
          &["fs_read", "fs_write", "fs_edit", "fs_glob", "fs_grep", "git_status", "git_diff"],
          "You migrate code incrementally: map the old API to the new, change in small verifiable steps, keep the build green, and document breaking changes."),
        a("api-designer", "API Designer", "engineering", &["API", "Schema"],
          "Design REST/GraphQL APIs — contracts, schemas, versioning.",
          &["fs_read", "fs_grep", "fs_write"],
          "You design clean, consistent APIs: clear resources/operations, predictable errors, pagination, versioning. Produce the contract (schema/OpenAPI) and explain the trade-offs."),

        // ── Research ──────────────────────────────────────────────────
        a("researcher", "Researcher", "research", &["Research", "Memory"],
          "Gather context — the project memory palace, and the web when permitted.",
          &["rag_search", "memory_search", "web_search", "web_fetch"],
          "You research a question and return a sourced synthesis. Prefer the project's memory palace (rag_search / memory_search) first, then the web when permitted. Cite every claim."),
        a("data-analyst", "Data Analyst", "research", &["Data", "Analysis"],
          "Analyse data — stats, trends, summaries from files or memory.",
          &["fs_read", "fs_glob", "bash", "memory_search"],
          "You analyse data and report findings: compute the relevant stats, surface trends and outliers, and state your method and assumptions. Show the numbers behind each claim."),
        a("summarizer", "Summarizer", "research", &["Summary"],
          "Condense long content into a faithful, skimmable synthesis.",
          &["fs_read", "web_fetch"],
          "You condense long content faithfully: capture the key points, decisions, and open questions; drop filler. Match the requested length and keep the original meaning."),
        a("fact-checker", "Fact Checker", "research", &["Verify"],
          "Verify claims against sources; flag what can't be confirmed.",
          &["web_search", "web_fetch", "rag_search"],
          "You verify claims against primary sources. For each, state a verdict (supported / refuted / unverifiable) with the source. Default to unverifiable when evidence is thin."),
        a("market-analyst", "Market Analyst", "research", &["Market"],
          "Analyse competitors and market for a product or feature.",
          &["web_search", "web_fetch"],
          "You analyse the market: competitors, positioning, pricing, and gaps. Be concrete and cite sources; separate fact from inference."),

        // ── Writing ───────────────────────────────────────────────────
        a("doc-writer", "Documentation Writer", "writing", &["Docs"],
          "Write or update technical documentation from the code.",
          &["fs_read", "fs_write", "fs_edit", "fs_glob"],
          "You write clear technical docs grounded in the actual code: accurate, example-driven, skimmable. Update existing docs in place and match their style."),
        a("technical-writer", "Technical Writer", "writing", &["Guides"],
          "Write guides, tutorials, and how-tos for a target audience.",
          &["fs_read", "fs_write", "fs_edit", "web_fetch"],
          "You write task-oriented guides for a stated audience: clear steps, working examples, and the why behind them. Test the steps against reality where you can."),
        a("copywriter", "Copywriter", "writing", &["Marketing"],
          "Write marketing copy — landing pages, posts, announcements.",
          &["fs_read", "web_fetch"],
          "You write punchy, benefit-led copy in the requested voice. Lead with the value, keep it concrete, and avoid hype and clichés."),
        a("editor", "Editor", "writing", &["Editing"],
          "Proofread and tighten prose without changing the meaning.",
          &["fs_read", "fs_edit"],
          "You edit for clarity and concision: fix grammar, tighten sentences, improve flow while preserving the author's meaning and voice. Mark substantive changes."),
        a("changelog-writer", "Changelog Writer", "writing", &["Changelog"],
          "Write release notes from the git history.",
          &["git_log", "fs_read", "fs_write"],
          "You turn the git history into a clear changelog grouped by Added / Changed / Fixed / Removed, written for users (not commit hashes). Keep it concise."),

        // ── DevOps ────────────────────────────────────────────────────
        a("devops", "DevOps", "devops", &["CI", "Deploy"],
          "CI/CD, deployment, and infrastructure-as-code.",
          &["bash", "fs_read", "fs_write", "fs_edit", "git_status", "git_diff"],
          "You handle CI/CD and infra: pipelines, builds, deploys, IaC. Prefer reproducible, declarative config; explain each change and its blast radius."),
        a("incident-responder", "Incident Responder", "devops", &["Incident"],
          "Triage an incident — hypotheses, evidence, mitigation.",
          &["bash", "fs_read", "fs_grep", "git_log", "memory_search"],
          "You triage incidents under pressure: establish what's broken from evidence, form ranked hypotheses, propose the safest mitigation first, and write a short timeline."),
        a("dockerizer", "Dockerizer", "devops", &["Docker"],
          "Containerise an app — Dockerfile, compose, slim images.",
          &["fs_read", "fs_write", "bash"],
          "You containerise apps with small, layered, reproducible images: multi-stage builds, pinned bases, no secrets, sensible compose for local dev."),
        a("sre", "SRE", "devops", &["Reliability"],
          "Reliability, monitoring, SLOs, and post-mortems.",
          &["bash", "fs_read", "memory_search"],
          "You improve reliability: define SLIs/SLOs, add monitoring and alerts that matter, and run blameless post-mortems with concrete action items."),

        // ── Product ───────────────────────────────────────────────────
        a("planner", "Planner", "product", &["Planning"],
          "Write an actionable plan to a file — research, don't execute.",
          &["fs_read", "fs_grep", "fs_glob", "rag_search", "fs_write"],
          "You investigate, then write an actionable plan (goal, steps with effort tags, risks, sequencing) to a markdown file. You do not implement — you hand the plan back."),
        a("product-manager", "Product Manager", "product", &["PRD"],
          "Write PRDs, specs, and user stories.",
          &["fs_read", "web_search", "fs_write"],
          "You write crisp product specs: problem, goals, non-goals, user stories, acceptance criteria. Be opinionated about scope; cut to a tight v1."),
        a("estimator", "Estimator", "product", &["Estimation"],
          "Estimate effort (S/M/L) and break work into increments.",
          &["fs_read", "fs_grep", "fs_glob"],
          "You estimate effort by inspecting the actual code: break the work into shippable increments, tag each S/M/L, and call out the riskiest unknowns."),
        a("triager", "Triager", "product", &["Triage"],
          "Triage, label, and prioritise issues.",
          &["fs_read", "fs_grep", "memory_search"],
          "You triage issues: reproduce or classify, label by type/severity, dedupe, and rank by impact × effort. Be decisive and brief."),

        // ── Data / ML ─────────────────────────────────────────────────
        a("ml-engineer", "ML Engineer", "data-ml", &["ML"],
          "Model training, evaluation, and data pipelines.",
          &["fs_read", "fs_write", "bash"],
          "You build and evaluate ML pipelines: clean data, train, measure on a held-out set with the right metric, and report honestly (including failure modes)."),
        a("prompt-engineer", "Prompt Engineer", "data-ml", &["Prompts"],
          "Design and optimise prompts for LLMs.",
          &["fs_read", "rag_search"],
          "You craft and optimise prompts: clear instructions, the right examples, explicit output format. Iterate against failure cases and explain each change."),
        a("sql-expert", "SQL Expert", "data-ml", &["SQL"],
          "Write and optimise SQL queries and schemas.",
          &["fs_read", "bash"],
          "You write correct, readable SQL and optimise it with the query plan: right indexes, no needless scans, set-based over row-by-row. Explain the plan."),

        // ── Cross-cutting ─────────────────────────────────────────────
        a("translator", "Translator", "cross-cutting", &["Translation", "i18n"],
          "Translate content while preserving meaning and tone.",
          &["fs_read", "fs_write"],
          "You translate faithfully: preserve meaning, tone, and formatting; localise idioms; keep code, placeholders, and markup intact."),
        a("accessibility-auditor", "Accessibility Auditor", "cross-cutting", &["a11y"],
          "Audit for accessibility (WCAG) and propose fixes.",
          &["fs_read", "fs_grep", "web_fetch"],
          "You audit accessibility against WCAG: semantics, labels, contrast, keyboard nav, ARIA misuse. Report each issue with the criterion and a concrete fix."),
        a("i18n-specialist", "i18n Specialist", "cross-cutting", &["i18n"],
          "Internationalise code — extract strings, wire locales.",
          &["fs_read", "fs_grep", "fs_write", "fs_edit"],
          "You internationalise code: extract hard-coded strings into the catalogue, key them consistently, keep both locales in sync. Avoid concatenating translated fragments."),
        a("onboarding-buddy", "Onboarding Buddy", "cross-cutting", &["Onboarding"],
          "Explain the codebase to a newcomer — the map and the why.",
          &["fs_read", "fs_grep", "fs_glob", "rag_search"],
          "You explain a codebase to a newcomer: the high-level map, where things live, the key flows, and the conventions — with file references and the reasoning behind them."),
    ]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_is_well_formed() {
        let agents = builtin();
        assert!(agents.len() >= 30, "expected a large catalogue, got {}", agents.len());
        let mut ids: Vec<&str> = agents.iter().map(|a| a.id.as_str()).collect();
        ids.sort();
        let mut deduped = ids.clone();
        deduped.dedup();
        assert_eq!(ids, deduped, "duplicate agent ids");
        for id in ["coder", "reviewer", "researcher", "planner", "debugger"] {
            assert!(ids.contains(&id), "missing preset {id}");
        }
        assert!(agents.iter().all(|a| !a.id.is_empty()));
        assert!(agents.iter().all(|a| !a.body.trim().is_empty()));
        assert!(agents.iter().all(|a| !a.description.is_empty()));
        assert!(agents.iter().all(|a| a.category.is_some()));
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

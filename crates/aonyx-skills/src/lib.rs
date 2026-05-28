//! # aonyx-skills
//!
//! Skill subsystem: parse [`SKILL.md`](https://agentskills.io) files, match them
//! against the current context, inject their system prompts, and (V1.2+)
//! auto-generate new skills from recurring task shapes.
//!
//! ## SKILL.md format (agentskills.io-compatible)
//!
//! ```markdown
//! ---
//! id: code-review
//! name: Code Review
//! enabled: true
//! tools: [fs_read, fs_grep, git_diff]
//! trigger:
//!   keywords: ["review", "lgtm", "look at this PR"]
//!   query_matches: ["^review the (PR|diff)"]
//!   project_matches: "^aonyx-.*"
//!   manual: false
//!   always_on: false
//! ---
//!
//! You are a meticulous code reviewer. Focus on correctness, then clarity, then
//! style. Cite line numbers when you raise an issue.
//! ```
//!
//! ## V1 built-in skills (ported from Aonyx RAG)
//! - `code-review`
//! - `data-analyst`
//! - `doc-writer`
//! - `incident-response`

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod engine;
pub mod loader;
pub mod schema;

pub use engine::SkillEngine;
pub use loader::SkillLoader;
pub use schema::{Skill, Trigger};

/// Return the V1 catalogue of built-in skills (`code-review`, `doc-writer`,
/// `data-analyst`, `incident-response`), parsed from the markdown files
/// embedded in the binary at compile time.
///
/// User-installed skills (e.g. under `~/.aonyx/skills/`) can be loaded
/// separately via [`SkillLoader::load_dir`] and concatenated to this list.
pub fn builtin_skills() -> Vec<Skill> {
    const SOURCES: [&str; 4] = [
        include_str!("../skills/built_in/code-review.skill.md"),
        include_str!("../skills/built_in/doc-writer.skill.md"),
        include_str!("../skills/built_in/data-analyst.skill.md"),
        include_str!("../skills/built_in/incident-response.skill.md"),
    ];
    SOURCES
        .iter()
        .filter_map(|raw| SkillLoader::parse(raw).ok())
        .collect()
}

#[cfg(test)]
mod lib_tests {
    use super::*;

    #[test]
    fn builtin_skills_loads_all_four() {
        let skills = builtin_skills();
        assert_eq!(skills.len(), 4);
        let mut ids: Vec<&str> = skills.iter().map(|s| s.id.as_str()).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec![
                "code-review",
                "data-analyst",
                "doc-writer",
                "incident-response"
            ]
        );
    }
}

//! # aonyx-skills
//!
//! Skill subsystem: parse [`SKILL.md`](https://agentskills.io) files, match them
//! against the current context, inject their system prompts, and
//! auto-generate new skills from recurring task shapes ([`miner`]).
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
pub mod miner;
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

/// Merge user-authored skills into a base catalogue (Phase DD).
///
/// A user skill whose `id` matches a base skill **overrides** it
/// (replacing it in place to preserve ordering); otherwise it is
/// appended. Use this to layer `~/.aonyx/skills/` on top of
/// [`builtin_skills`].
pub fn merge_skills(mut base: Vec<Skill>, overlay: Vec<Skill>) -> Vec<Skill> {
    for s in overlay {
        if let Some(slot) = base.iter_mut().find(|b| b.id == s.id) {
            *slot = s;
        } else {
            base.push(s);
        }
    }
    base
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

    fn skill(id: &str, name: &str) -> Skill {
        Skill {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            category: None,
            tags: Vec::new(),
            version: None,
            author: None,
            enabled: true,
            tools: Vec::new(),
            trigger: Trigger::default(),
            body: String::new(),
        }
    }

    #[test]
    fn merge_skills_appends_new_ids() {
        let base = vec![skill("a", "A")];
        let merged = merge_skills(base, vec![skill("b", "B")]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "a");
        assert_eq!(merged[1].id, "b");
    }

    #[test]
    fn merge_skills_overrides_same_id_in_place() {
        let base = vec![skill("a", "A original"), skill("b", "B")];
        let merged = merge_skills(base, vec![skill("a", "A overridden")]);
        assert_eq!(merged.len(), 2);
        // Override keeps position 0 and replaces the name.
        assert_eq!(merged[0].id, "a");
        assert_eq!(merged[0].name, "A overridden");
        assert_eq!(merged[1].id, "b");
    }

    #[test]
    fn merge_skills_with_empty_overlay_is_identity() {
        let base = vec![skill("a", "A")];
        let merged = merge_skills(base, vec![]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "a");
    }
}

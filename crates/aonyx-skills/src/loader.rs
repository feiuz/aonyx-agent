//! Walk a directory tree for `SKILL.md` (and `*.skill.md`) files and parse them.
//!
//! Port reference: Aonyx RAG `rag_system/skills/loader.py`.
//!
//! A SKILL.md file is a markdown document whose **frontmatter** is YAML
//! delimited by `---` lines:
//!
//! ```markdown
//! ---
//! id: code-review
//! name: Code Review
//! enabled: true
//! tools: [fs_read, fs_grep]
//! trigger:
//!   keywords: ["review", "lgtm"]
//!   query_matches: ["(?i)review the (pr|diff)"]
//!   manual: false
//!   always_on: false
//! ---
//!
//! You are a meticulous code reviewer.
//! ```

use std::path::Path;

use aonyx_core::{AonyxError, Result};
use walkdir::WalkDir;

use crate::schema::Skill;

/// Skill loader.
pub struct SkillLoader;

impl SkillLoader {
    /// Recursively load every `SKILL.md` and `*.skill.md` file under `dir`.
    ///
    /// Files that fail to parse are skipped with a `tracing::warn!`; this
    /// matches the Aonyx RAG behaviour where a single broken skill should not
    /// disable the rest.
    pub fn load_dir(dir: impl AsRef<Path>) -> Result<Vec<Skill>> {
        let mut out = Vec::new();
        for entry in WalkDir::new(dir.as_ref())
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name != "SKILL.md" && !name.ends_with(".skill.md") {
                continue;
            }
            match Self::load_file(path) {
                Ok(skill) => out.push(skill),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "skipping malformed skill");
                }
            }
        }
        Ok(out)
    }

    /// Read and parse a single skill file.
    pub fn load_file(path: impl AsRef<Path>) -> Result<Skill> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path)
            .map_err(|e| AonyxError::Skill(format!("read {}: {e}", path.display())))?;
        let mut skill = Self::parse(&raw)
            .map_err(|e| AonyxError::Skill(format!("parse {}: {e}", path.display())))?;
        // Infer the catalogue category from a `<category>/<skill>/SKILL.md` layout.
        if skill.category.is_none() {
            skill.category = category_from_path(path);
        }
        Ok(skill)
    }

    /// Parse a SKILL.md from its raw text.
    pub fn parse(raw: &str) -> Result<Skill> {
        let (frontmatter, body) = split_frontmatter(raw)
            .ok_or_else(|| AonyxError::Skill("missing or malformed YAML frontmatter".into()))?;
        let mut skill: Skill = serde_yaml::from_str(frontmatter)
            .map_err(|e| AonyxError::Skill(format!("parse frontmatter: {e}")))?;
        skill.body = body.to_string();
        normalize(&mut skill);
        Ok(skill)
    }
}

/// Fill the Aonyx-native fields a portable (Hermes-style) skill omits: derive
/// `id` from `name`, and — when no trigger is given — activate on the skill's
/// tags + id words so it still surfaces on relevant queries.
fn normalize(skill: &mut Skill) {
    if skill.id.trim().is_empty() {
        skill.id = slugify(&skill.name);
    }
    let t = &skill.trigger;
    let untriggered = t.keywords.is_empty()
        && t.query_matches.is_empty()
        && t.project_matches.is_none()
        && !t.manual
        && !t.always_on;
    if untriggered {
        skill.trigger.keywords = derive_keywords(skill);
    }
}

/// Lowercase, hyphenate, collapse — `"GitHub PR Workflow"` → `"github-pr-workflow"`.
fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Keywords for a skill without an explicit trigger: its tags plus the
/// significant words of its id.
fn derive_keywords(skill: &Skill) -> Vec<String> {
    let mut kws: Vec<String> = Vec::new();
    let mut push = |w: String| {
        if !w.is_empty() && !kws.contains(&w) {
            kws.push(w);
        }
    };
    for tag in &skill.tags {
        push(tag.to_lowercase());
    }
    for word in skill.id.split('-') {
        if word.len() > 3 {
            push(word.to_lowercase());
        }
    }
    kws
}

/// Infer the catalogue category from a `…/<category>/<skill>/SKILL.md` layout.
/// Returns `None` for a flat `<name>.skill.md` file or an un-nested skill.
fn category_from_path(path: &Path) -> Option<String> {
    if path.file_name()?.to_str()? != "SKILL.md" {
        return None;
    }
    let cat = path.parent()?.parent()?.file_name()?.to_str()?;
    if cat == "skills" || cat == "built_in" {
        None
    } else {
        Some(cat.to_string())
    }
}

/// Split a raw SKILL.md text into `(frontmatter, body)`.
///
/// Frontmatter starts at byte 0 with `---\n` (or `---\r\n`) and ends on the
/// next line that contains exactly `---` followed by a newline or EOF. The
/// returned body is trimmed of any leading newlines.
fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let after_start = raw
        .strip_prefix("---\r\n")
        .or_else(|| raw.strip_prefix("---\n"))?;

    let mut cursor = 0;
    while let Some(rel) = after_start[cursor..].find("\n---") {
        let abs = cursor + rel;
        let after = &after_start[abs + 4..];
        let next_is_nl_or_eof =
            after.is_empty() || after.starts_with('\n') || after.starts_with('\r');
        if next_is_nl_or_eof {
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
    use std::path::PathBuf;

    fn builtin_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills/built_in")
    }

    #[test]
    fn parses_minimal_valid_skill() {
        let raw = r#"---
id: hello
name: Hello
enabled: true
trigger:
  keywords: ["hi"]
---

You say hi back.
"#;
        let skill = SkillLoader::parse(raw).unwrap();
        assert_eq!(skill.id, "hello");
        assert_eq!(skill.name, "Hello");
        assert!(skill.enabled);
        assert_eq!(skill.trigger.keywords, vec!["hi"]);
        assert!(skill.body.contains("You say hi back."));
    }

    #[test]
    fn parses_portable_hermes_skill() {
        // No `id`, no `trigger`, plus portable fields — id + keywords are derived
        // and unknown keys (version/author/license/platforms/metadata) are ignored.
        let raw = r#"---
name: GitHub PR Workflow
description: "GitHub PR lifecycle: branch, commit, open, CI, merge."
version: 1.0.0
author: Hermes Agent
license: MIT
platforms: [linux, macos, windows]
tags: [GitHub, PR, Review]
metadata:
  hermes:
    related_skills: [github-issues]
---

Open a PR, watch CI, merge.
"#;
        let skill = SkillLoader::parse(raw).unwrap();
        assert_eq!(skill.id, "github-pr-workflow");
        assert_eq!(
            skill.description.as_deref(),
            Some("GitHub PR lifecycle: branch, commit, open, CI, merge.")
        );
        assert_eq!(skill.author.as_deref(), Some("Hermes Agent"));
        assert!(skill.tags.contains(&"GitHub".to_string()));
        // keywords derived from tags + id words
        assert!(skill.trigger.keywords.contains(&"github".to_string()));
        assert!(skill.trigger.keywords.contains(&"workflow".to_string()));
    }

    #[test]
    fn rejects_text_without_frontmatter() {
        let err = SkillLoader::parse("just text\n").unwrap_err();
        assert!(format!("{err}").contains("frontmatter"));
    }

    #[test]
    fn rejects_frontmatter_without_closing_marker() {
        let raw = "---\nid: x\nname: X\n";
        assert!(SkillLoader::parse(raw).is_err());
    }

    #[test]
    fn loads_every_builtin_skill() {
        let skills = SkillLoader::load_dir(builtin_dir()).unwrap();
        let mut ids: Vec<&str> = skills.iter().map(|s| s.id.as_str()).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec![
                "code-review",
                "data-analyst",
                "doc-writer",
                "incident-response",
                "plan",
                "systematic-debugging",
            ]
        );
    }

    #[test]
    fn builtin_skills_have_non_empty_bodies() {
        let skills = SkillLoader::load_dir(builtin_dir()).unwrap();
        for s in &skills {
            assert!(!s.body.trim().is_empty(), "skill {} body empty", s.id);
        }
    }
}

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
        Self::parse(&raw).map_err(|e| AonyxError::Skill(format!("parse {}: {e}", path.display())))
    }

    /// Parse a SKILL.md from its raw text.
    pub fn parse(raw: &str) -> Result<Skill> {
        let (frontmatter, body) = split_frontmatter(raw)
            .ok_or_else(|| AonyxError::Skill("missing or malformed YAML frontmatter".into()))?;
        let mut skill: Skill = serde_yaml::from_str(frontmatter)
            .map_err(|e| AonyxError::Skill(format!("parse frontmatter: {e}")))?;
        skill.body = body.to_string();
        Ok(skill)
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
                "incident-response"
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

//! Trigger matching + system-prompt injection engine.
//!
//! Port reference: Aonyx RAG `rag_system/skills/engine.py`.
//!
//! Activation precedence (per skill, highest priority first):
//! 1. `enabled = false` → always inactive.
//! 2. `always_on = true` → always active.
//! 3. `manual = true` → only active when explicitly invoked (not via this engine).
//! 4. Keyword match (case-insensitive substring against the query).
//! 5. Query regex match.
//! 6. Project regex match.
//!
//! Skills that fail to compile their regexes are silently skipped — a broken
//! pattern in one skill should not disable the rest of the catalogue.

use regex::Regex;

use crate::schema::Skill;

/// Activation engine over a static skill catalogue.
pub struct SkillEngine {
    skills: Vec<Skill>,
}

impl SkillEngine {
    /// Build an engine from a list of skills (typically the loader output).
    pub fn new(skills: Vec<Skill>) -> Self {
        Self { skills }
    }

    /// Snapshot every known skill (active or not).
    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// Number of registered skills.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// `true` when no skill is registered.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Return every skill currently active given the user query + project context.
    pub fn match_active(&self, query: &str, project: Option<&str>) -> Vec<&Skill> {
        let query_lower = query.to_lowercase();
        let mut out = Vec::new();
        for skill in &self.skills {
            if !skill.enabled {
                continue;
            }
            if skill.trigger.always_on {
                out.push(skill);
                continue;
            }
            if skill.trigger.manual {
                continue;
            }

            let keyword_hit = skill
                .trigger
                .keywords
                .iter()
                .any(|kw| query_lower.contains(&kw.to_lowercase()));

            let query_regex_hit = skill
                .trigger
                .query_matches
                .iter()
                .any(|pat| Regex::new(pat).map(|r| r.is_match(query)).unwrap_or(false));

            let project_regex_hit = match (&skill.trigger.project_matches, project) {
                (Some(pat), Some(p)) => Regex::new(pat).map(|r| r.is_match(p)).unwrap_or(false),
                _ => false,
            };

            if keyword_hit || query_regex_hit || project_regex_hit {
                out.push(skill);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Trigger;

    fn skill_with_trigger(id: &str, trigger: Trigger) -> Skill {
        Skill {
            id: id.into(),
            name: id.into(),
            description: None,
            category: None,
            tags: Vec::new(),
            version: None,
            author: None,
            enabled: true,
            tools: Vec::new(),
            trigger,
            body: "body".into(),
        }
    }

    #[test]
    fn disabled_skills_are_never_active() {
        let mut s = skill_with_trigger(
            "x",
            Trigger {
                always_on: true,
                ..Trigger::default()
            },
        );
        s.enabled = false;
        let engine = SkillEngine::new(vec![s]);
        assert!(engine.match_active("anything", None).is_empty());
    }

    #[test]
    fn always_on_skills_are_always_active() {
        let t = Trigger {
            always_on: true,
            ..Trigger::default()
        };
        let engine = SkillEngine::new(vec![skill_with_trigger("ao", t)]);
        let hits = engine.match_active("", None);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "ao");
    }

    #[test]
    fn manual_skills_never_auto_match() {
        let t = Trigger {
            manual: true,
            keywords: vec!["x".into()],
            ..Trigger::default()
        };
        let engine = SkillEngine::new(vec![skill_with_trigger("m", t)]);
        assert!(engine.match_active("xxx", None).is_empty());
    }

    #[test]
    fn keyword_match_is_case_insensitive() {
        let t = Trigger {
            keywords: vec!["Review".into()],
            ..Trigger::default()
        };
        let engine = SkillEngine::new(vec![skill_with_trigger("r", t)]);
        assert_eq!(engine.match_active("please review the PR", None).len(), 1);
        assert_eq!(engine.match_active("PLEASE REVIEW", None).len(), 1);
        assert!(engine.match_active("nothing here", None).is_empty());
    }

    #[test]
    fn query_regex_match_works() {
        let t = Trigger {
            query_matches: vec!["(?i)^analyze the data$".into()],
            ..Trigger::default()
        };
        let engine = SkillEngine::new(vec![skill_with_trigger("a", t)]);
        assert_eq!(engine.match_active("Analyze the data", None).len(), 1);
        assert!(engine.match_active("not at all", None).is_empty());
    }

    #[test]
    fn project_regex_match_works() {
        let t = Trigger {
            project_matches: Some("^aonyx-.*".into()),
            ..Trigger::default()
        };
        let engine = SkillEngine::new(vec![skill_with_trigger("p", t)]);
        assert_eq!(
            engine.match_active("anything", Some("aonyx-agent")).len(),
            1
        );
        assert!(engine.match_active("anything", Some("ovelo")).is_empty());
        assert!(engine.match_active("anything", None).is_empty());
    }

    #[test]
    fn invalid_regex_is_silently_skipped() {
        let t = Trigger {
            query_matches: vec!["[invalid".into()],
            ..Trigger::default()
        };
        let engine = SkillEngine::new(vec![skill_with_trigger("bad", t)]);
        // No panic, no match — the malformed regex just doesn't fire.
        assert!(engine.match_active("anything", None).is_empty());
    }
}

//! Skill auto-generation (Phase XX) — mine recurring request shapes and
//! write a `SKILL.md` once a shape recurs `threshold` times. Enabled by
//! default; the generated skill is picked up on the next session by
//! [`crate::SkillLoader::load_dir`].
//!
//! The "shape" of a request is approximated by its leading meaningful word
//! (usually the action verb): three "review …" requests generate a
//! `review` skill seeded with the actual examples seen. It is intentionally
//! coarse and deterministic — no model call — so it is cheap and testable.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

const STATS_FILE: &str = "skill_stats.json";
const MAX_EXAMPLES: usize = 5;

/// Words too generic to be a task signature.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "you", "your", "that", "this", "with", "from", "into", "please", "can",
    "could", "would", "should", "what", "when", "where", "which", "about", "there", "their",
    "have", "has", "had", "are", "was", "were", "will", "just", "let", "its", "our", "out", "get",
    "got", "not", "but", "all", "any", "use", "make", "new", "aonyx", "agent",
];

#[derive(Default, Serialize, Deserialize)]
struct Stats {
    #[serde(default)]
    shapes: BTreeMap<String, Shape>,
}

#[derive(Default, Serialize, Deserialize)]
struct Shape {
    count: usize,
    #[serde(default)]
    examples: Vec<String>,
    #[serde(default)]
    generated: bool,
}

/// Observe one user request. When its shape newly reaches `threshold`
/// occurrences and no skill file for it exists yet, generate a `SKILL.md`
/// under `<config_dir>/skills/` and return the new skill's id. Best-effort:
/// returns `None` on any I/O hiccup or when the request has no usable
/// signature.
pub fn observe(config_dir: &Path, request: &str, threshold: usize) -> Option<String> {
    let threshold = threshold.max(2);
    let sig = signature(request)?;
    let stats_path = config_dir.join(STATS_FILE);
    let mut stats = load_stats(&stats_path);

    let shape = stats.shapes.entry(sig.clone()).or_default();
    shape.count += 1;
    let req = request.trim().to_string();
    if shape.examples.len() < MAX_EXAMPLES && !shape.examples.contains(&req) {
        shape.examples.push(req);
    }

    let mut generated = None;
    if !shape.generated && shape.count >= threshold {
        let id = format!("auto-{sig}");
        let skills_dir = config_dir.join("skills");
        let file = skills_dir.join(format!("{id}.skill.md"));
        if file.exists() {
            // The user already has (or kept) this skill — don't clobber.
            shape.generated = true;
        } else if std::fs::create_dir_all(&skills_dir).is_ok()
            && std::fs::write(&file, render_skill(&sig, &shape.examples)).is_ok()
        {
            shape.generated = true;
            generated = Some(id);
        }
        // On a write failure `generated` stays false, so we retry next time.
    }

    let _ = save_stats(&stats_path, &stats);
    generated
}

/// The leading meaningful word of a request (lowercased), or `None` when
/// it is all stopwords / punctuation.
fn signature(request: &str) -> Option<String> {
    request
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .find(|w| w.len() >= 3 && !STOPWORDS.contains(&w.as_str()))
}

fn load_stats(path: &Path) -> Stats {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_stats(path: &Path, stats: &Stats) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(stats).unwrap_or_default();
    std::fs::write(path, json)
}

fn render_skill(sig: &str, examples: &[String]) -> String {
    let title = capitalize(sig);
    let mut bullets = String::new();
    for ex in examples {
        bullets.push_str("- ");
        bullets.push_str(ex);
        bullets.push('\n');
    }
    format!(
        "---\n\
         id: auto-{sig}\n\
         name: \"{title} tasks (auto)\"\n\
         enabled: true\n\
         trigger:\n  keywords:\n    - {sig}\n\
         ---\n\n\
         # {title} tasks\n\n\
         You frequently ask Aonyx to **{sig}** things — this skill was generated \
         automatically from your usage. Recent examples:\n\n\
         {bullets}\n\
         When a request matches this shape, reuse what worked before: clarify scope, \
         follow the prior steps, and cite sources from the memory palace. Edit or delete \
         this file in `~/.aonyx/skills/` to customise it.\n"
    )
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_picks_leading_action_word() {
        assert_eq!(
            signature("Review this PR for bugs").as_deref(),
            Some("review")
        );
        assert_eq!(
            signature("please refactor the parser").as_deref(),
            Some("refactor")
        );
        assert_eq!(signature("???  ...").as_deref(), None);
    }

    #[test]
    fn generates_after_threshold_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        assert_eq!(observe(p, "review the auth module", 3), None);
        assert_eq!(observe(p, "review the parser", 3), None);
        let id = observe(p, "review the config", 3);
        assert_eq!(id.as_deref(), Some("auto-review"));
        assert!(p.join("skills").join("auto-review.skill.md").exists());
        // No duplicate generation afterwards.
        assert_eq!(observe(p, "review once more", 3), None);
    }

    #[test]
    fn generated_skill_parses_as_a_valid_skill() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        for _ in 0..3 {
            observe(p, "deploy the service", 3);
        }
        let raw = std::fs::read_to_string(p.join("skills").join("auto-deploy.skill.md")).unwrap();
        let skill = crate::SkillLoader::parse(&raw).expect("generated SKILL.md must parse");
        assert_eq!(skill.id, "auto-deploy");
        assert!(skill.trigger.keywords.contains(&"deploy".to_string()));
        assert!(!skill.body.is_empty());
    }
}

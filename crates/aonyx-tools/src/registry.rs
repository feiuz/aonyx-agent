//! Tool registry — name-keyed dispatch with schema introspection.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use aonyx_core::ToolHandler;

use crate::bash::Bash;
use crate::fs::{FsEdit, FsGlob, FsGrep, FsRead, FsWrite};
use crate::git::{GitDiff, GitLog, GitShow, GitStatus};
use crate::web::{WebFetch, WebSearch};

/// A registry of registered [`ToolHandler`]s keyed by name.
///
/// The `disabled` set lives behind an `Arc<Mutex<_>>` so every clone of
/// the registry shares the same on/off state — that's how the TUI's
/// `/tools` panel (Phase Q) can flip a tool live and have the runner
/// pick up the change.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
    disabled: Arc<Mutex<HashSet<String>>>,
}

impl ToolRegistry {
    /// Build an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool handler. Existing entries with the same name are replaced.
    pub fn register(&mut self, handler: Arc<dyn ToolHandler>) {
        self.handlers.insert(handler.name().to_string(), handler);
    }

    /// Look up a tool by name. Returns `None` for both unknown and
    /// disabled tools — the runner treats both the same way (drops the
    /// call).
    pub fn get(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        if self.is_disabled(name) {
            return None;
        }
        self.handlers.get(name).cloned()
    }

    /// Like [`get`] but ignores the disabled flag — used by the
    /// `/tools` panel (Phase Q) to enumerate every registered tool
    /// regardless of its on/off state.
    pub fn get_raw(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        self.handlers.get(name).cloned()
    }

    /// Enumerate registered tool names (unordered).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.handlers.keys().map(String::as_str)
    }

    /// Count registered tools (does not subtract disabled ones).
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// `true` when no handler is registered.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Returns `true` when `name` is currently disabled.
    pub fn is_disabled(&self, name: &str) -> bool {
        self.disabled
            .lock()
            .map(|d| d.contains(name))
            .unwrap_or(false)
    }

    /// Disable `name` for every clone of this registry. No-op when the
    /// tool is already disabled or not registered.
    pub fn disable(&self, name: &str) {
        if let Ok(mut d) = self.disabled.lock() {
            d.insert(name.to_string());
        }
    }

    /// Enable `name`. No-op when the tool was not disabled.
    pub fn enable(&self, name: &str) {
        if let Ok(mut d) = self.disabled.lock() {
            d.remove(name);
        }
    }

    /// Flip the disabled state of `name`. Returns the new state
    /// (`true` = now disabled).
    pub fn toggle(&self, name: &str) -> bool {
        if let Ok(mut d) = self.disabled.lock() {
            if d.contains(name) {
                d.remove(name);
                false
            } else {
                d.insert(name.to_string());
                true
            }
        } else {
            false
        }
    }

    /// Build the V1 default registry: every fs / bash / git built-in.
    pub fn default_set() -> Self {
        let mut r = Self::new();
        r.register(Arc::new(FsRead));
        r.register(Arc::new(FsWrite));
        r.register(Arc::new(FsEdit));
        r.register(Arc::new(FsGlob));
        r.register(Arc::new(FsGrep));
        r.register(Arc::new(Bash));
        r.register(Arc::new(GitStatus));
        r.register(Arc::new(GitDiff));
        r.register(Arc::new(GitLog));
        r.register(Arc::new(GitShow));
        r.register(Arc::new(WebFetch));
        r.register(Arc::new(WebSearch));
        r
    }

    /// Build an **isolated** registry containing only the tools whose names
    /// match `patterns` — exact names, a trailing-`*` prefix (`git_*`), or a
    /// bare `*` for everything. An empty pattern list inherits every enabled
    /// tool. The returned registry has its own disabled set, so toggling it
    /// never touches the parent — used to scope a sub-agent's toolset (ADR-017).
    pub fn subset(&self, patterns: &[String]) -> ToolRegistry {
        let keep = |name: &str| -> bool {
            if patterns.is_empty() {
                return true;
            }
            patterns.iter().any(|p| {
                if p == "*" {
                    true
                } else if let Some(prefix) = p.strip_suffix('*') {
                    name.starts_with(prefix)
                } else {
                    name == p
                }
            })
        };
        let mut out = ToolRegistry::new();
        for (name, h) in &self.handlers {
            if keep(name) && !self.is_disabled(name) {
                out.handlers.insert(name.clone(), Arc::clone(h));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_set_registers_every_v1_tool() {
        let r = ToolRegistry::default_set();
        let mut names: Vec<&str> = r.names().collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "bash",
                "fs_edit",
                "fs_glob",
                "fs_grep",
                "fs_read",
                "fs_write",
                "git_diff",
                "git_log",
                "git_show",
                "git_status",
                "web_fetch",
                "web_search",
            ]
        );
        assert_eq!(r.len(), 12);
    }

    #[test]
    fn get_returns_none_for_unknown_tool() {
        let r = ToolRegistry::default_set();
        assert!(r.get("does_not_exist").is_none());
        assert!(r.get("bash").is_some());
    }

    #[test]
    fn disable_hides_tool_from_get_but_not_from_names() {
        let r = ToolRegistry::default_set();
        r.disable("bash");
        assert!(r.is_disabled("bash"));
        assert!(r.get("bash").is_none());
        assert!(r.get_raw("bash").is_some());
        let names: Vec<&str> = r.names().collect();
        assert!(names.contains(&"bash"));
    }

    #[test]
    fn enable_after_disable_restores_visibility() {
        let r = ToolRegistry::default_set();
        r.disable("bash");
        r.enable("bash");
        assert!(!r.is_disabled("bash"));
        assert!(r.get("bash").is_some());
    }

    #[test]
    fn toggle_flips_and_returns_new_state() {
        let r = ToolRegistry::default_set();
        assert!(r.toggle("bash")); // now disabled
        assert!(r.is_disabled("bash"));
        assert!(!r.toggle("bash")); // re-enabled
        assert!(!r.is_disabled("bash"));
    }

    #[test]
    fn disabled_state_is_shared_across_clones() {
        let a = ToolRegistry::default_set();
        let b = a.clone();
        a.disable("bash");
        assert!(b.is_disabled("bash"));
        assert!(b.get("bash").is_none());
    }

    #[test]
    fn subset_filters_by_exact_and_wildcard() {
        let r = ToolRegistry::default_set();
        let sub = r.subset(&["bash".to_string(), "git_*".to_string()]);
        let mut names: Vec<&str> = sub.names().collect();
        names.sort();
        assert_eq!(
            names,
            vec!["bash", "git_diff", "git_log", "git_show", "git_status"]
        );
        assert!(sub.get("fs_read").is_none());
        // Empty patterns inherit the whole toolset.
        assert_eq!(r.subset(&[]).len(), r.len());
    }

    #[test]
    fn subset_has_an_isolated_disabled_set() {
        let r = ToolRegistry::default_set();
        let sub = r.subset(&[]);
        sub.disable("bash");
        assert!(sub.is_disabled("bash"));
        assert!(!r.is_disabled("bash")); // parent untouched — no shared toggle leak
    }
}

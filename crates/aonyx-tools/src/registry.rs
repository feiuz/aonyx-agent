//! Tool registry — name-keyed dispatch with schema introspection.

use std::collections::HashMap;
use std::sync::Arc;

use aonyx_core::ToolHandler;

use crate::bash::Bash;
use crate::fs::{FsEdit, FsGlob, FsGrep, FsRead, FsWrite};
use crate::git::{GitDiff, GitLog, GitShow, GitStatus};

/// A registry of registered [`ToolHandler`]s keyed by name.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    handlers: HashMap<String, Arc<dyn ToolHandler>>,
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

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        self.handlers.get(name).cloned()
    }

    /// Enumerate registered tool names (unordered).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.handlers.keys().map(String::as_str)
    }

    /// Count registered tools.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// `true` when no handler is registered.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
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
        r
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
            ]
        );
        assert_eq!(r.len(), 10);
    }

    #[test]
    fn get_returns_none_for_unknown_tool() {
        let r = ToolRegistry::default_set();
        assert!(r.get("does_not_exist").is_none());
        assert!(r.get("bash").is_some());
    }
}

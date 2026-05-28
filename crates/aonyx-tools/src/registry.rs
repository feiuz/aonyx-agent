//! Tool registry — name-keyed dispatch with schema introspection.

use std::collections::HashMap;
use std::sync::Arc;

use aonyx_core::ToolHandler;

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

    /// Enumerate registered tool names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.handlers.keys().map(String::as_str)
    }

    /// Build the V1 default registry: fs, bash, git, exec, web, memory tools.
    ///
    /// TODO(V1): populate with concrete handlers as they land.
    pub fn default_set() -> Self {
        Self::new()
    }
}

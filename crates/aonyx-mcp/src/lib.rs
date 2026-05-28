//! # aonyx-mcp
//!
//! Model Context Protocol — both directions.
//!
//! - [`client`] — connect to external MCP servers (stdio / HTTP / SSE), discover
//!   their tools, and register them into the Aonyx [`ToolRegistry`](aonyx_tools::ToolRegistry).
//! - [`server`] — expose Aonyx's own tool catalogue + memory tools to other
//!   agents (Claude Code, Cursor, Cline, …).
//!
//! Auth: Bearer token from `~/.aonyx/mcp_tokens.toml`.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod client;
pub mod server;

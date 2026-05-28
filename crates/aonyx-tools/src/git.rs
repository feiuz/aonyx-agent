//! Read-only git tools: status, diff, log, show, blame.
//!
//! Writing to history (`commit`, `push`, `rebase`) lives behind the bash tool
//! so that the approval gate sees a single, auditable command.

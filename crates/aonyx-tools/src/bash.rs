//! Sandboxed shell invocation with timeout.
//!
//! Safety class: `Caution` for read-only commands, `Destructive` otherwise.
//! The classifier inspects the command before invocation.

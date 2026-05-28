//! Filesystem tools: read, write, edit, glob, grep.
//!
//! Safety classes:
//! - `fs_read`, `fs_glob`, `fs_grep` → `Safe`
//! - `fs_write`, `fs_edit` → `Destructive` (route through approval gate)

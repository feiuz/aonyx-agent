//! Append-only journal of file snapshots taken before destructive `fs_*`
//! tool calls — drives `/undo` (Phase J).
//!
//! Format: one JSON object per line in `<cwd>/.aonyx/undo.jsonl`. Each
//! entry captures the path that was about to be mutated and the file's
//! contents *before* mutation (`None` when the file did not exist yet).
//!
//! The journal is single-process and lock-free on purpose — Aonyx is a
//! single-user CLI and concurrent writes from the same project would be
//! anomalous.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// One reversible mutation of a file on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoSnapshot {
    /// Path the tool was about to touch, as supplied to the tool.
    pub path: String,
    /// File contents before mutation, or `None` if the file did not
    /// exist (in which case restore = delete).
    pub prior: Option<String>,
    /// Which tool emitted the snapshot (`fs_edit` or `fs_write`).
    pub tool: String,
    /// Unix seconds at the moment of capture.
    pub ts: i64,
}

/// Default journal location — `<cwd>/.aonyx/undo.jsonl`.
pub fn journal_path() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.join(".aonyx").join("undo.jsonl")
}

/// Capture the current state of `path` and append it to the default
/// journal.
///
/// Best-effort: a failure to journal never blocks the actual tool
/// invocation. The caller should `let _ =` the result.
pub fn append_snapshot(snap: UndoSnapshot) -> std::io::Result<()> {
    append_snapshot_to(&journal_path(), snap)
}

/// Append a snapshot to a specific journal file. Public for tests and
/// any callers that need to isolate the journal from `cwd`.
pub fn append_snapshot_to(journal: &Path, snap: UndoSnapshot) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = journal.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(&snap).map_err(std::io::Error::other)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal)?;
    writeln!(f, "{line}")?;
    Ok(())
}

/// Pop the most recent snapshot off the default journal. Returns `None`
/// when the journal is missing or empty. Removes the file when the last
/// line is drained.
pub fn pop_last_snapshot() -> std::io::Result<Option<UndoSnapshot>> {
    pop_last_snapshot_from(&journal_path())
}

/// Pop the most recent snapshot off a specific journal file.
pub fn pop_last_snapshot_from(journal: &Path) -> std::io::Result<Option<UndoSnapshot>> {
    if !journal.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(journal)?;
    let mut lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
    let Some(last_line) = lines.pop() else {
        return Ok(None);
    };
    let snap: UndoSnapshot = serde_json::from_str(last_line).map_err(std::io::Error::other)?;
    if lines.is_empty() {
        let _ = std::fs::remove_file(journal);
    } else {
        let mut new_content = lines.join("\n");
        new_content.push('\n');
        std::fs::write(journal, new_content)?;
    }
    Ok(Some(snap))
}

/// Apply an [`UndoSnapshot`]: write `prior` back to `path`, or delete the
/// file when there was no prior state.
pub fn restore(snap: &UndoSnapshot) -> std::io::Result<()> {
    match &snap.prior {
        Some(content) => {
            if let Some(parent) = Path::new(&snap.path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(&snap.path, content)
        }
        None => {
            if Path::new(&snap.path).exists() {
                std::fs::remove_file(&snap.path)
            } else {
                Ok(())
            }
        }
    }
}

/// Convenience constructor populating the timestamp from the system
/// clock.
pub fn snapshot(path: impl Into<String>, prior: Option<String>, tool: &str) -> UndoSnapshot {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    UndoSnapshot {
        path: path.into(),
        prior,
        tool: tool.to_string(),
        ts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn pop_returns_none_when_no_journal() {
        let dir = TempDir::new().unwrap();
        let j = dir.path().join("undo.jsonl");
        assert!(pop_last_snapshot_from(&j).unwrap().is_none());
    }

    #[test]
    fn append_then_pop_round_trips_snapshot() {
        let dir = TempDir::new().unwrap();
        let j = dir.path().join("undo.jsonl");
        append_snapshot_to(&j, snapshot("foo.rs", Some("before".into()), "fs_edit")).unwrap();
        let popped = pop_last_snapshot_from(&j).unwrap().expect("some");
        assert_eq!(popped.path, "foo.rs");
        assert_eq!(popped.prior.as_deref(), Some("before"));
        assert_eq!(popped.tool, "fs_edit");
        assert!(pop_last_snapshot_from(&j).unwrap().is_none());
    }

    #[test]
    fn pop_returns_lifo_order() {
        let dir = TempDir::new().unwrap();
        let j = dir.path().join("undo.jsonl");
        append_snapshot_to(&j, snapshot("a", Some("a0".into()), "fs_edit")).unwrap();
        append_snapshot_to(&j, snapshot("b", Some("b0".into()), "fs_edit")).unwrap();
        append_snapshot_to(&j, snapshot("c", Some("c0".into()), "fs_edit")).unwrap();
        assert_eq!(pop_last_snapshot_from(&j).unwrap().unwrap().path, "c");
        assert_eq!(pop_last_snapshot_from(&j).unwrap().unwrap().path, "b");
        assert_eq!(pop_last_snapshot_from(&j).unwrap().unwrap().path, "a");
        assert!(pop_last_snapshot_from(&j).unwrap().is_none());
    }

    #[test]
    fn restore_writes_prior_back_to_disk() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("hello.txt");
        std::fs::write(&target, "after").unwrap();
        let snap = snapshot(target.to_string_lossy(), Some("before".into()), "fs_edit");
        restore(&snap).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "before");
    }

    #[test]
    fn restore_deletes_file_when_prior_is_none() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("new.txt");
        std::fs::write(&target, "newly created").unwrap();
        let snap = snapshot(target.to_string_lossy(), None, "fs_write");
        restore(&snap).unwrap();
        assert!(!target.exists());
    }
}

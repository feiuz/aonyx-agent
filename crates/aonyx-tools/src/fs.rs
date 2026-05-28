//! Filesystem tools: read, write, edit, glob, grep.
//!
//! Safety classes:
//! - `fs_read`, `fs_glob`, `fs_grep` → [`SafetyClass::Safe`]
//! - `fs_write`, `fs_edit` → [`SafetyClass::Destructive`] (must clear the approval gate)

use std::path::{Path, PathBuf};

use aonyx_core::{AonyxError, Result, SafetyClass, ToolCall, ToolHandler, ToolResult};
use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use walkdir::WalkDir;

/// `fs_read` — read a UTF-8 text file in full. Safe.
pub struct FsRead;

#[derive(Deserialize)]
struct FsReadArgs {
    path: String,
}

#[async_trait]
impl ToolHandler for FsRead {
    fn name(&self) -> &str {
        "fs_read"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or workspace-relative path to a UTF-8 text file."
                }
            },
            "required": ["path"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: FsReadArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("fs_read args: {e}")))?;
        let content = tokio::fs::read_to_string(&args.path)
            .await
            .map_err(|e| AonyxError::Tool(format!("fs_read {}: {e}", args.path)))?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "path": args.path, "content": content }),
            error: None,
        })
    }
}

/// `fs_write` — overwrite (or create) a file. Destructive.
pub struct FsWrite;

#[derive(Deserialize)]
struct FsWriteArgs {
    path: String,
    content: String,
}

#[async_trait]
impl ToolHandler for FsWrite {
    fn name(&self) -> &str {
        "fs_write"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Destructive
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: FsWriteArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("fs_write args: {e}")))?;
        if let Some(parent) = Path::new(&args.path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| AonyxError::Tool(format!("fs_write mkdir {parent:?}: {e}")))?;
            }
        }
        let bytes = args.content.len();
        tokio::fs::write(&args.path, args.content.as_bytes())
            .await
            .map_err(|e| AonyxError::Tool(format!("fs_write {}: {e}", args.path)))?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "path": args.path, "bytes_written": bytes }),
            error: None,
        })
    }
}

/// `fs_edit` — exact-string replacement inside a file. Destructive.
///
/// Mirrors Claude Code's `Edit` tool semantics: the substring `old_string`
/// must appear **exactly once** in the file; it is replaced by `new_string`.
pub struct FsEdit;

#[derive(Deserialize)]
struct FsEditArgs {
    path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

#[async_trait]
impl ToolHandler for FsEdit {
    fn name(&self) -> &str {
        "fs_edit"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Destructive
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old_string": { "type": "string", "description": "Exact substring to find. Must be unique unless replace_all=true." },
                "new_string": { "type": "string" },
                "replace_all": { "type": "boolean", "default": false }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: FsEditArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("fs_edit args: {e}")))?;
        let original = tokio::fs::read_to_string(&args.path)
            .await
            .map_err(|e| AonyxError::Tool(format!("fs_edit read {}: {e}", args.path)))?;

        let occurrences = original.matches(&args.old_string).count();
        let new_text = if args.replace_all {
            original.replace(&args.old_string, &args.new_string)
        } else {
            if occurrences == 0 {
                return Err(AonyxError::Tool(format!(
                    "fs_edit {}: old_string not found",
                    args.path
                )));
            }
            if occurrences > 1 {
                return Err(AonyxError::Tool(format!(
                    "fs_edit {}: old_string is ambiguous ({} occurrences); pass replace_all or widen context",
                    args.path, occurrences
                )));
            }
            original.replacen(&args.old_string, &args.new_string, 1)
        };

        tokio::fs::write(&args.path, new_text.as_bytes())
            .await
            .map_err(|e| AonyxError::Tool(format!("fs_edit write {}: {e}", args.path)))?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({
                "path": args.path,
                "replacements": if args.replace_all { occurrences } else { 1 }
            }),
            error: None,
        })
    }
}

/// `fs_glob` — list files matching a glob pattern. Safe.
pub struct FsGlob;

#[derive(Deserialize)]
struct FsGlobArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

#[async_trait]
impl ToolHandler for FsGlob {
    fn name(&self) -> &str {
        "fs_glob"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern, e.g. 'src/**/*.rs'." },
                "path": { "type": "string", "description": "Base directory (default: cwd)." }
            },
            "required": ["pattern"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: FsGlobArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("fs_glob args: {e}")))?;
        let combined = match &args.path {
            Some(p) => format!("{}/{}", p.trim_end_matches('/'), args.pattern),
            None => args.pattern.clone(),
        };
        let mut hits: Vec<String> = Vec::new();
        for entry in
            glob::glob(&combined).map_err(|e| AonyxError::Tool(format!("fs_glob pattern: {e}")))?
        {
            match entry {
                Ok(p) => hits.push(p.to_string_lossy().into_owned()),
                Err(e) => return Err(AonyxError::Tool(format!("fs_glob walk: {e}"))),
            }
        }
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "pattern": combined, "matches": hits }),
            error: None,
        })
    }
}

/// `fs_grep` — search file contents for a regex. Safe.
pub struct FsGrep;

#[derive(Deserialize)]
struct FsGrepArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    file_pattern: Option<String>,
    #[serde(default)]
    max_results: Option<usize>,
}

#[async_trait]
impl ToolHandler for FsGrep {
    fn name(&self) -> &str {
        "fs_grep"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex applied per line." },
                "path": { "type": "string", "description": "Directory to walk (default: cwd)." },
                "file_pattern": { "type": "string", "description": "Optional regex on filenames to keep." },
                "max_results": { "type": "integer", "minimum": 1 }
            },
            "required": ["pattern"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: FsGrepArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("fs_grep args: {e}")))?;
        let re = Regex::new(&args.pattern)
            .map_err(|e| AonyxError::Tool(format!("fs_grep regex: {e}")))?;
        let file_re = args
            .file_pattern
            .as_deref()
            .map(Regex::new)
            .transpose()
            .map_err(|e| AonyxError::Tool(format!("fs_grep file_pattern: {e}")))?;
        let base: PathBuf = args.path.map(PathBuf::from).unwrap_or_else(|| ".".into());
        let cap = args.max_results.unwrap_or(200);

        let hits = tokio::task::spawn_blocking(move || -> Result<Vec<Value>> {
            let mut out: Vec<Value> = Vec::new();
            for entry in WalkDir::new(&base)
                .follow_links(false)
                .into_iter()
                .flatten()
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                if let Some(fr) = &file_re {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                    if !name.as_deref().map(|n| fr.is_match(n)).unwrap_or(false) {
                        continue;
                    }
                }
                let text = match std::fs::read_to_string(path) {
                    Ok(t) => t,
                    Err(_) => continue, // skip binary or unreadable files
                };
                for (idx, line) in text.lines().enumerate() {
                    if re.is_match(line) {
                        out.push(json!({
                            "path": path.to_string_lossy(),
                            "line_number": idx + 1,
                            "line": line,
                        }));
                        if out.len() >= cap {
                            return Ok(out);
                        }
                    }
                }
            }
            Ok(out)
        })
        .await
        .map_err(|e| AonyxError::Tool(format!("fs_grep join: {e}")))??;

        Ok(ToolResult {
            call_id: call.id,
            output: json!({
                "pattern": args.pattern,
                "matches": hits,
            }),
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use uuid::Uuid;

    fn call(name: &str, args: Value) -> ToolCall {
        ToolCall {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            args,
        }
    }

    #[tokio::test]
    async fn fs_read_returns_file_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hello.txt");
        tokio::fs::write(&path, "hello aonyx").await.unwrap();
        let res = FsRead
            .invoke(call("fs_read", json!({ "path": path.to_string_lossy() })))
            .await
            .unwrap();
        assert_eq!(res.output["content"], "hello aonyx");
    }

    #[tokio::test]
    async fn fs_write_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b/c.txt");
        FsWrite
            .invoke(call(
                "fs_write",
                json!({ "path": nested.to_string_lossy(), "content": "hi" }),
            ))
            .await
            .unwrap();
        let body = tokio::fs::read_to_string(&nested).await.unwrap();
        assert_eq!(body, "hi");
    }

    #[tokio::test]
    async fn fs_edit_single_match_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        tokio::fs::write(&path, "alpha beta gamma").await.unwrap();
        FsEdit
            .invoke(call(
                "fs_edit",
                json!({
                    "path": path.to_string_lossy(),
                    "old_string": "beta",
                    "new_string": "BETA",
                }),
            ))
            .await
            .unwrap();
        let body = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(body, "alpha BETA gamma");
    }

    #[tokio::test]
    async fn fs_edit_ambiguous_match_fails_without_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        tokio::fs::write(&path, "x x x").await.unwrap();
        let err = FsEdit
            .invoke(call(
                "fs_edit",
                json!({
                    "path": path.to_string_lossy(),
                    "old_string": "x",
                    "new_string": "y",
                }),
            ))
            .await
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("ambiguous"), "got: {msg}");
    }

    #[tokio::test]
    async fn fs_edit_replace_all_rewrites_every_occurrence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        tokio::fs::write(&path, "x x x").await.unwrap();
        let res = FsEdit
            .invoke(call(
                "fs_edit",
                json!({
                    "path": path.to_string_lossy(),
                    "old_string": "x",
                    "new_string": "y",
                    "replace_all": true,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(res.output["replacements"], 3);
        let body = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(body, "y y y");
    }

    #[tokio::test]
    async fn fs_glob_matches_pattern() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.rs"), "").await.unwrap();
        tokio::fs::write(dir.path().join("b.rs"), "").await.unwrap();
        tokio::fs::write(dir.path().join("c.txt"), "")
            .await
            .unwrap();

        let pattern = format!("{}/*.rs", dir.path().to_string_lossy().replace('\\', "/"));
        let res = FsGlob
            .invoke(call("fs_glob", json!({ "pattern": pattern })))
            .await
            .unwrap();
        let n = res.output["matches"].as_array().unwrap().len();
        assert_eq!(n, 2);
    }

    #[tokio::test]
    async fn fs_grep_finds_matching_lines() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("a.txt"), "foo\nbar\nfoo bar\n")
            .await
            .unwrap();
        let res = FsGrep
            .invoke(call(
                "fs_grep",
                json!({
                    "pattern": "bar",
                    "path": dir.path().to_string_lossy(),
                }),
            ))
            .await
            .unwrap();
        let n = res.output["matches"].as_array().unwrap().len();
        assert_eq!(n, 2);
    }
}

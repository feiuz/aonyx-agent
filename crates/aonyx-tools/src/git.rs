//! Read-only git tools: status, diff, log, show.
//!
//! Mutating operations (`commit`, `push`, `rebase`, …) deliberately live behind
//! the [`crate::bash::Bash`] tool so the approval gate sees a single, auditable
//! command rather than a parameterised git wrapper.

use std::time::Duration;

use aonyx_core::{AonyxError, Result, SafetyClass, ToolCall, ToolHandler, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;

const TIMEOUT: Duration = Duration::from_secs(30);

async fn run_git(args: &[&str], cwd: Option<&str>) -> Result<(i32, String, String)> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.kill_on_drop(true);

    let output = tokio::time::timeout(TIMEOUT, cmd.output())
        .await
        .map_err(|_| AonyxError::Tool(format!("git {args:?} timed out")))?
        .map_err(|e| AonyxError::Tool(format!("git spawn: {e}")))?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

fn cwd_schema_field() -> Value {
    json!({ "type": "string", "description": "Repository root (default: cwd)." })
}

/// `git_status` — porcelain status.
pub struct GitStatus;

#[derive(Deserialize)]
struct CwdArg {
    #[serde(default)]
    cwd: Option<String>,
}

#[async_trait]
impl ToolHandler for GitStatus {
    fn name(&self) -> &str {
        "git_status"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "cwd": cwd_schema_field() },
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: CwdArg = serde_json::from_value(call.args).unwrap_or(CwdArg { cwd: None });
        let (code, stdout, stderr) = run_git(
            &["status", "--porcelain=v1", "--branch"],
            args.cwd.as_deref(),
        )
        .await?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "exit_code": code, "stdout": stdout, "stderr": stderr }),
            error: None,
        })
    }
}

/// `git_diff` — unified diff between two refs (default: working tree vs HEAD).
pub struct GitDiff;

#[derive(Deserialize)]
struct GitDiffArgs {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    head: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
}

#[async_trait]
impl ToolHandler for GitDiff {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "cwd": cwd_schema_field(),
                "base": { "type": "string", "description": "Base ref (default: HEAD)." },
                "head": { "type": "string", "description": "Head ref (default: working tree)." },
                "paths": { "type": "array", "items": { "type": "string" } }
            }
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: GitDiffArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("git_diff args: {e}")))?;
        let mut cli: Vec<&str> = vec!["diff", "--no-color"];
        if let Some(b) = &args.base {
            cli.push(b);
        }
        if let Some(h) = &args.head {
            cli.push(h);
        }
        if !args.paths.is_empty() {
            cli.push("--");
            for p in &args.paths {
                cli.push(p);
            }
        }
        let (code, stdout, stderr) = run_git(&cli, args.cwd.as_deref()).await?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "exit_code": code, "stdout": stdout, "stderr": stderr }),
            error: None,
        })
    }
}

/// `git_log` — oneline log, optionally limited.
pub struct GitLog;

#[derive(Deserialize)]
struct GitLogArgs {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default = "GitLogArgs::default_limit")]
    limit: u32,
    #[serde(default)]
    paths: Vec<String>,
}

impl GitLogArgs {
    fn default_limit() -> u32 {
        20
    }
}

#[async_trait]
impl ToolHandler for GitLog {
    fn name(&self) -> &str {
        "git_log"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "cwd": cwd_schema_field(),
                "limit": { "type": "integer", "minimum": 1, "default": 20 },
                "paths": { "type": "array", "items": { "type": "string" } }
            }
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: GitLogArgs = serde_json::from_value(call.args).unwrap_or(GitLogArgs {
            cwd: None,
            limit: GitLogArgs::default_limit(),
            paths: Vec::new(),
        });
        let limit_arg = format!("-{}", args.limit);
        let mut cli: Vec<&str> = vec!["log", "--oneline", "--no-color", &limit_arg];
        if !args.paths.is_empty() {
            cli.push("--");
            for p in &args.paths {
                cli.push(p);
            }
        }
        let (code, stdout, stderr) = run_git(&cli, args.cwd.as_deref()).await?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "exit_code": code, "stdout": stdout, "stderr": stderr }),
            error: None,
        })
    }
}

/// `git_show` — show a commit (default: HEAD).
pub struct GitShow;

#[derive(Deserialize)]
struct GitShowArgs {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    rev: Option<String>,
}

#[async_trait]
impl ToolHandler for GitShow {
    fn name(&self) -> &str {
        "git_show"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Safe
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "cwd": cwd_schema_field(),
                "rev": { "type": "string", "default": "HEAD" }
            }
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: GitShowArgs = serde_json::from_value(call.args).unwrap_or(GitShowArgs {
            cwd: None,
            rev: None,
        });
        let rev = args.rev.unwrap_or_else(|| "HEAD".to_string());
        let (code, stdout, stderr) =
            run_git(&["show", "--no-color", &rev], args.cwd.as_deref()).await?;
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "exit_code": code, "rev": rev, "stdout": stdout, "stderr": stderr }),
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

    async fn git_init_tempdir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy().to_string();
        Bash.invoke(call(
            "bash",
            json!({
                "command": "git init -q -b main && git config user.name t && git config user.email t@t",
                "cwd": path
            }),
        ))
        .await
        .unwrap();
        dir
    }

    #[tokio::test]
    async fn git_status_runs_on_fresh_repo() {
        let dir = git_init_tempdir().await;
        let res = GitStatus
            .invoke(call(
                "git_status",
                json!({ "cwd": dir.path().to_string_lossy() }),
            ))
            .await
            .unwrap();
        assert_eq!(res.output["exit_code"], 0);
        let out = res.output["stdout"].as_str().unwrap_or("");
        assert!(out.contains("main"), "got: {out}");
    }

    #[tokio::test]
    async fn git_log_returns_zero_commits_on_empty_repo() {
        let dir = git_init_tempdir().await;
        let res = GitLog
            .invoke(call(
                "git_log",
                json!({ "cwd": dir.path().to_string_lossy(), "limit": 5 }),
            ))
            .await
            .unwrap();
        // Empty repo → git log exits non-zero, but the tool still returns a
        // structured ToolResult capturing exit_code + stderr.
        assert!(res.output["exit_code"].as_i64().unwrap() != 0);
    }

    use super::super::bash::Bash;
}

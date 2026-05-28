//! Sandboxed shell invocation with timeout.
//!
//! V1 routing:
//! - Linux/macOS: `sh -c "<command>"`
//! - Windows: `cmd /C "<command>"`
//!
//! `bash` is always classified as [`SafetyClass::Destructive`] in V1: the
//! approval gate inspects the command string before invocation. A smarter
//! classifier (read-only vs side-effecting commands) lands in V1.1.

use std::time::Duration;

use aonyx_core::{AonyxError, Result, SafetyClass, ToolCall, ToolHandler, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;

const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// `bash` — run a shell command and capture stdout / stderr / exit code.
pub struct Bash;

#[derive(Deserialize)]
struct BashArgs {
    command: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    cwd: Option<String>,
}

#[async_trait]
impl ToolHandler for Bash {
    fn name(&self) -> &str {
        "bash"
    }

    fn classify(&self) -> SafetyClass {
        SafetyClass::Destructive
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command to execute." },
                "timeout_secs": { "type": "integer", "minimum": 1, "default": DEFAULT_TIMEOUT_SECS },
                "cwd": { "type": "string", "description": "Working directory; defaults to current." }
            },
            "required": ["command"]
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: BashArgs = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("bash args: {e}")))?;

        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", &args.command]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", &args.command]);
            c
        };
        if let Some(dir) = &args.cwd {
            cmd.current_dir(dir);
        }
        cmd.kill_on_drop(true);

        let timeout = Duration::from_secs(args.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS));
        let output = tokio::time::timeout(timeout, cmd.output())
            .await
            .map_err(|_| AonyxError::Tool(format!("bash timed out after {timeout:?}")))?
            .map_err(|e| AonyxError::Tool(format!("bash spawn: {e}")))?;

        Ok(ToolResult {
            call_id: call.id,
            output: json!({
                "command": args.command,
                "exit_code": output.status.code(),
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
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

    fn call(args: Value) -> ToolCall {
        ToolCall {
            id: Uuid::new_v4().to_string(),
            name: "bash".to_string(),
            args,
        }
    }

    #[tokio::test]
    async fn bash_runs_a_trivial_command() {
        // `cargo --version` exists on any host where these tests can run.
        let res = Bash
            .invoke(call(json!({ "command": "cargo --version" })))
            .await
            .unwrap();
        assert_eq!(res.output["exit_code"], 0);
        let stdout = res.output["stdout"].as_str().unwrap_or("");
        assert!(stdout.starts_with("cargo "), "got: {stdout}");
    }

    #[tokio::test]
    async fn bash_reports_nonzero_exit() {
        // A command guaranteed to fail on every platform.
        let cmd = if cfg!(windows) {
            "exit 7"
        } else {
            "false; exit 7"
        };
        let res = Bash.invoke(call(json!({ "command": cmd }))).await.unwrap();
        assert_eq!(res.output["exit_code"], 7);
    }

    #[tokio::test]
    async fn bash_times_out_when_command_hangs() {
        // sleep 5 seconds with a 1-second timeout.
        let cmd = if cfg!(windows) {
            "ping -n 6 127.0.0.1 > NUL"
        } else {
            "sleep 5"
        };
        let err = Bash
            .invoke(call(json!({ "command": cmd, "timeout_secs": 1 })))
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("timed out"));
    }
}

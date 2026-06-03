//! Sandboxed / remote command execution (Phase CCC).
//!
//! `sandbox_exec` runs a shell command **off the host**:
//! - **Docker** backend — `docker run --rm <image> sh -c <cmd>` (an
//!   ephemeral, isolated local sandbox).
//! - **HTTP** backend — `POST <url> {"command": "..."}` returning
//!   `{stdout, stderr, exit_code}`. Point it at a Modal web function, a
//!   Daytona workspace-exec endpoint, or any shim with that contract.
//!
//! The tool is registered only when a backend is configured. It is light
//! (no new dependencies — `tokio` process + `reqwest`).

use std::time::Duration;

use aonyx_core::{AonyxError, Result, SafetyClass, ToolCall, ToolHandler, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Where `sandbox_exec` runs commands.
enum Backend {
    /// Ephemeral Docker container (`docker run --rm <image> sh -c <cmd>`).
    Docker { image: String },
    /// Remote sandbox over HTTP (Modal / Daytona / any compatible shim).
    Http { url: String, token: Option<String> },
}

/// `sandbox_exec` — run a command in the configured sandbox backend.
pub struct SandboxExec {
    backend: Backend,
    timeout_secs: u64,
}

impl SandboxExec {
    /// Build from config. Returns `None` when no usable backend is
    /// configured (so the tool isn't registered).
    pub fn from_config(
        backend: Option<&str>,
        image: Option<String>,
        url: Option<String>,
        token: Option<String>,
    ) -> Option<Self> {
        let backend = match backend? {
            "docker" => Backend::Docker {
                image: image.unwrap_or_else(|| "alpine".to_string()),
            },
            "http" => Backend::Http { url: url?, token },
            _ => return None,
        };
        Some(Self {
            backend,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        })
    }

    /// Human label of the active backend (for the tool description).
    fn label(&self) -> String {
        match &self.backend {
            Backend::Docker { image } => format!("docker:{image}"),
            Backend::Http { url, .. } => format!("http:{url}"),
        }
    }
}

#[derive(Deserialize)]
struct Args {
    command: String,
}

#[async_trait]
impl ToolHandler for SandboxExec {
    fn name(&self) -> &str {
        "sandbox_exec"
    }
    fn classify(&self) -> SafetyClass {
        // Executes arbitrary commands — gate it like `bash`, even though
        // the sandbox isolates them from the host.
        SafetyClass::Destructive
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "description": format!("Run a shell command in an isolated sandbox ({}). Returns stdout, stderr, exit_code.", self.label()),
            "properties": { "command": { "type": "string" } },
            "required": ["command"]
        })
    }
    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let args: Args = serde_json::from_value(call.args)
            .map_err(|e| AonyxError::Tool(format!("sandbox_exec args: {e}")))?;
        let (stdout, stderr, code) = match &self.backend {
            Backend::Docker { image } => run_docker(image, &args.command, self.timeout_secs).await,
            Backend::Http { url, token } => run_http(url, token.as_deref(), &args.command).await,
        };
        Ok(ToolResult {
            call_id: call.id,
            output: json!({ "stdout": stdout, "stderr": stderr, "exit_code": code }),
            error: None,
        })
    }
}

async fn run_docker(image: &str, cmd: &str, timeout_secs: u64) -> (String, String, i32) {
    let mut command = tokio::process::Command::new("docker");
    command
        .arg("run")
        .arg("--rm")
        .arg(image)
        .arg("sh")
        .arg("-c")
        .arg(cmd)
        .kill_on_drop(true);
    match tokio::time::timeout(Duration::from_secs(timeout_secs), command.output()).await {
        Ok(Ok(out)) => (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code().unwrap_or(-1),
        ),
        Ok(Err(e)) => (
            String::new(),
            format!("docker backend: {e} (is Docker installed and on PATH?)"),
            -1,
        ),
        Err(_) => (
            String::new(),
            format!("sandbox command timed out after {timeout_secs}s"),
            -1,
        ),
    }
}

async fn run_http(url: &str, token: Option<&str>, cmd: &str) -> (String, String, i32) {
    let mut req = reqwest::Client::new()
        .post(url)
        .json(&json!({ "command": cmd }));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return (String::new(), format!("sandbox HTTP request: {e}"), -1),
    };
    let status = resp.status();
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return (String::new(), format!("sandbox HTTP decode: {e}"), -1),
    };
    if !status.is_success() {
        return (String::new(), format!("sandbox HTTP {status}: {body}"), -1);
    }
    let stdout = body
        .get("stdout")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let stderr = body
        .get("stderr")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let code = body.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    (stdout, stderr, code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_config_selects_backend() {
        assert!(SandboxExec::from_config(None, None, None, None).is_none());
        assert!(SandboxExec::from_config(Some("nonsense"), None, None, None).is_none());
        // http needs a url
        assert!(SandboxExec::from_config(Some("http"), None, None, None).is_none());

        let d = SandboxExec::from_config(Some("docker"), None, None, None).unwrap();
        assert_eq!(d.label(), "docker:alpine");
        assert_eq!(d.name(), "sandbox_exec");
        assert!(matches!(d.classify(), SafetyClass::Destructive));

        let h = SandboxExec::from_config(
            Some("http"),
            None,
            Some("https://sb.example/exec".into()),
            None,
        )
        .unwrap();
        assert_eq!(h.label(), "http:https://sb.example/exec");
    }
}

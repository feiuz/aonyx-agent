//! Aonyx Agent desktop (Tauri 2).
//!
//! Vague 4 / V4.6 — a thin GUI over the **automation API** (`aonyx serve
//! api`). The frontend never talks HTTP directly (avoids CORS and keeps the
//! webview sandboxed); instead these Tauri commands proxy requests with
//! `reqwest` from the Rust side. Blocking turns ship in V4.6; streaming
//! (SSE/WS) and an embedded local agent land in V4.7–V4.8.

use serde_json::Value;

fn join(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

/// Issue an HTTP request to the API and return the decoded JSON body.
async fn send(
    method: reqwest::Method,
    url: String,
    token: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let mut rb = client.request(method, &url);
    if !token.is_empty() {
        rb = rb.bearer_auth(token);
    }
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let resp = rb
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    finish(resp).await
}

/// Decode an API response into JSON (or a descriptive error).
async fn finish(resp: reqwest::Response) -> Result<Value, String> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {status}: {text}"));
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| format!("bad JSON from server: {e}"))
}

/// `GET /v1/info` — server identity + capabilities (also a connection probe).
#[tauri::command]
async fn api_info(base: String, token: String) -> Result<Value, String> {
    send(reqwest::Method::GET, join(&base, "/v1/info"), &token, None).await
}

/// `POST /v1/sessions` — create a session.
#[tauri::command]
async fn api_create_session(
    base: String,
    token: String,
    project: Option<String>,
) -> Result<Value, String> {
    let body = serde_json::json!({ "project": project });
    send(
        reqwest::Method::POST,
        join(&base, "/v1/sessions"),
        &token,
        Some(body),
    )
    .await
}

/// `POST /v1/approvals/{id}` — resolve a paused destructive tool call.
#[tauri::command]
async fn api_approve(
    base: String,
    token: String,
    id: String,
    approved: bool,
) -> Result<Value, String> {
    send(
        reqwest::Method::POST,
        join(&base, &format!("/v1/approvals/{id}")),
        &token,
        Some(serde_json::json!({ "approved": approved })),
    )
    .await
}

/// `POST /v1/sessions/{id}/messages` — run one blocking turn.
#[tauri::command]
async fn api_send(
    base: String,
    token: String,
    session: String,
    content: String,
) -> Result<Value, String> {
    let path = format!("/v1/sessions/{session}/messages");
    let body = serde_json::json!({ "content": content });
    send(
        reqwest::Method::POST,
        join(&base, &path),
        &token,
        Some(body),
    )
    .await
}

/// `POST /v1/sessions/{id}/messages/stream` — run one turn, relaying each
/// SSE `StreamFrame` to the frontend over a Tauri channel as it arrives.
#[tauri::command]
async fn api_stream(
    base: String,
    token: String,
    session: String,
    content: String,
    on_event: tauri::ipc::Channel<Value>,
) -> Result<(), String> {
    use futures_util::StreamExt;

    let url = join(&base, &format!("/v1/sessions/{session}/messages/stream"));
    let client = reqwest::Client::new();
    let mut rb = client
        .post(&url)
        .json(&serde_json::json!({ "content": content }));
    if !token.is_empty() {
        rb = rb.bearer_auth(token);
    }
    let resp = rb
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {text}"));
    }

    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|e| format!("stream error: {e}"))?;
        buf.push_str(std::str::from_utf8(&bytes).unwrap_or(""));
        // SSE events are separated by a blank line.
        while let Some(idx) = buf.find("\n\n") {
            let block = buf[..idx].to_string();
            buf.drain(..(idx + 2));
            for line in block.lines() {
                if let Some(p) = line.strip_prefix("data:") {
                    if let Ok(frame) = serde_json::from_str::<Value>(p.trim_start()) {
                        let _ = on_event.send(frame);
                    }
                }
            }
        }
    }
    Ok(())
}

/// `GET /v1/sessions` — list recent sessions for a project.
#[tauri::command]
async fn api_list_sessions(
    base: String,
    token: String,
    project: Option<String>,
) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let mut rb = client.get(join(&base, "/v1/sessions"));
    if let Some(p) = project.filter(|p| !p.is_empty()) {
        rb = rb.query(&[("project", p)]);
    }
    if !token.is_empty() {
        rb = rb.bearer_auth(token);
    }
    finish(
        rb.send()
            .await
            .map_err(|e| format!("request failed: {e}"))?,
    )
    .await
}

/// `GET /v1/sessions/{id}` — full record including the message log.
#[tauri::command]
async fn api_get_session(base: String, token: String, session: String) -> Result<Value, String> {
    send(
        reqwest::Method::GET,
        join(&base, &format!("/v1/sessions/{session}")),
        &token,
        None,
    )
    .await
}

/// `GET /v1/memory/search` — hybrid memory-palace search.
#[tauri::command]
async fn api_memory_search(
    base: String,
    token: String,
    q: String,
    k: Option<usize>,
) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let mut rb = client
        .get(join(&base, "/v1/memory/search"))
        .query(&[("q", q.as_str()), ("k", &k.unwrap_or(8).to_string())]);
    if !token.is_empty() {
        rb = rb.bearer_auth(token);
    }
    finish(
        rb.send()
            .await
            .map_err(|e| format!("request failed: {e}"))?,
    )
    .await
}

/// `GET /v1/memory/kg/entities` — knowledge-graph entities (nodes).
#[tauri::command]
async fn api_kg_entities(base: String, token: String, limit: Option<u32>) -> Result<Value, String> {
    let l = limit.unwrap_or(200);
    send(
        reqwest::Method::GET,
        join(&base, &format!("/v1/memory/kg/entities?limit={l}")),
        &token,
        None,
    )
    .await
}

/// `GET /v1/memory/kg/relations` — knowledge-graph relations (edges).
#[tauri::command]
async fn api_kg_relations(base: String, token: String, limit: Option<u32>) -> Result<Value, String> {
    let l = limit.unwrap_or(500);
    send(
        reqwest::Method::GET,
        join(&base, &format!("/v1/memory/kg/relations?limit={l}")),
        &token,
        None,
    )
    .await
}

/// `GET /v1/tools` — registered tools (built-in, MCP, plugin).
#[tauri::command]
async fn api_tools(base: String, token: String) -> Result<Value, String> {
    send(reqwest::Method::GET, join(&base, "/v1/tools"), &token, None).await
}

/// List the models a provider actually exposes — a **live** query, not a
/// hardcoded list: ollama `/api/tags`, OpenAI-compatible `/v1/models`,
/// OpenRouter's catalogue, Anthropic `/v1/models`. Claude Code reuses its own
/// stored OAuth session token (`~/.claude/.credentials.json`) against the same
/// Anthropic Models API — verified to return the real list. Returns the marker
/// `API_KEY_REQUIRED` when a key-based provider has no key, so the UI prompts.
#[tauri::command]
async fn list_models(provider: String, base: String, key: String) -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();
    let trim = |b: &str| b.trim_end_matches('/').to_string();
    let req = match provider.as_str() {
        "ollama" => {
            let b = if base.is_empty() {
                "http://localhost:11434".to_string()
            } else {
                base
            };
            client.get(format!("{}/api/tags", trim(&b)))
        }
        "lm-studio" => {
            let b = if base.is_empty() {
                "http://localhost:1234".to_string()
            } else {
                base
            };
            client.get(format!("{}/v1/models", trim(&b)))
        }
        "openai" => {
            if key.trim().is_empty() {
                return Err("API_KEY_REQUIRED".to_string());
            }
            let b = if base.is_empty() {
                "https://api.openai.com".to_string()
            } else {
                base
            };
            client.get(format!("{}/v1/models", trim(&b))).bearer_auth(key)
        }
        "openrouter" => client.get("https://openrouter.ai/api/v1/models"),
        "anthropic" => {
            if key.trim().is_empty() {
                return Err("API_KEY_REQUIRED".to_string());
            }
            client
                .get("https://api.anthropic.com/v1/models?limit=1000")
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
        }
        "claude-code" => {
            // Reuse Claude Code's own OAuth session token
            // (~/.claude/.credentials.json) against the Anthropic Models API,
            // else fall back to ANTHROPIC_API_KEY. The stored token can be expired
            // (Claude Code rotates it) — detect that and ask the user to refresh
            // instead of firing a doomed request that returns a raw 401.
            match read_claude_code_auth() {
                ClaudeCodeAuth::Token(token) => client
                    .get("https://api.anthropic.com/v1/models?limit=1000")
                    .header("authorization", format!("Bearer {token}"))
                    .header("anthropic-version", "2023-06-01"),
                other => {
                    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
                        client
                            .get("https://api.anthropic.com/v1/models?limit=1000")
                            .header("x-api-key", api_key)
                            .header("anthropic-version", "2023-06-01")
                    } else if matches!(other, ClaudeCodeAuth::Expired) {
                        return Err("CLAUDE_CODE_EXPIRED".to_string());
                    } else {
                        return Err("CLAUDE_CODE_ABSENT".to_string());
                    }
                }
            }
        }
        other => return Err(format!("unknown provider: {other}")),
    };
    let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("HTTP {status}: {snippet}"));
    }
    let json: Value = serde_json::from_str(&body).map_err(|e| format!("bad JSON: {e}"))?;
    let mut models: Vec<String> = if provider == "ollama" {
        json.get("models")
            .and_then(|m| m.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        // OpenAI-shaped: { "data": [ { "id": "..." }, … ] }.
        json.get("data")
            .and_then(|d| d.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    models.sort();
    models.dedup();
    Ok(models)
}

/// Launch the Claude Code CLI so the user can refresh / re-login its OAuth
/// session. Claude Code owns its own credentials — on startup it silently
/// refreshes an expired access token (and prompts `/login` only if the refresh
/// token is also dead), then persists fresh creds the desktop re-reads. We never
/// reimplement Anthropic's OAuth nor write `~/.claude/.credentials.json`.
#[tauri::command]
fn claude_login(binary: Option<String>) -> Result<(), String> {
    let bin = binary
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| "claude".to_string());
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "Aonyx - reconnexion Claude Code", "cmd", "/K", &bin])
            .spawn()
            .map_err(|e| format!("could not launch `{bin}`: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-a", "Terminal", &bin])
            .spawn()
            .map_err(|e| format!("could not launch Terminal: {e}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let term = std::env::var("TERMINAL").unwrap_or_else(|_| "x-terminal-emulator".to_string());
        std::process::Command::new(term)
            .args(["-e", &bin])
            .spawn()
            .map_err(|e| format!("could not launch a terminal: {e}"))?;
    }
    Ok(())
}

/// Claude Code OAuth state read from `~/.claude/.credentials.json`.
enum ClaudeCodeAuth {
    /// A non-empty access token whose `expiresAt` is still in the future.
    Token(String),
    /// A token exists but `expiresAt` is in the past — Claude Code must refresh
    /// it (`claude` / `/login`); sending it would only earn a 401.
    Expired,
    /// No usable token on disk.
    Absent,
}

/// Read Claude Code's stored OAuth session (`~/.claude/.credentials.json`, key
/// `claudeAiOauth`) so models can be listed from the Anthropic Models API using
/// the user's existing Claude Code session — no separate key. Honours
/// `expiresAt` (unix ms) so we never fire a request with a stale token.
fn read_claude_code_auth() -> ClaudeCodeAuth {
    let home = match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        Some(h) => h,
        None => return ClaudeCodeAuth::Absent,
    };
    let path = std::path::Path::new(&home)
        .join(".claude")
        .join(".credentials.json");
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(_) => return ClaudeCodeAuth::Absent,
    };
    let json: Value = match serde_json::from_str(&data) {
        Ok(j) => j,
        Err(_) => return ClaudeCodeAuth::Absent,
    };
    let oauth = match json.get("claudeAiOauth") {
        Some(o) => o,
        None => return ClaudeCodeAuth::Absent,
    };
    let token = oauth
        .get("accessToken")
        .and_then(|t| t.as_str())
        .unwrap_or("");
    if token.is_empty() {
        return ClaudeCodeAuth::Absent;
    }
    if let Some(exp) = oauth.get("expiresAt").and_then(|e| e.as_i64()) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if exp <= now {
            return ClaudeCodeAuth::Expired;
        }
    }
    ClaudeCodeAuth::Token(token.to_string())
}

/// Holds the managed local `aonyx serve api` child, if one is running.
#[derive(Default)]
struct LocalAgent(std::sync::Mutex<Option<std::process::Child>>);

/// Pick a free loopback TCP port (best-effort; falls back to 8788).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(8788)
}

/// Resolve the `aonyx` agent binary: prefer the sidecar bundled next to the app
/// executable (prod: Tauri's `externalBin`; dev: staged into `target/<profile>/`
/// by `scripts/stage-sidecar.sh`), else fall back to `aonyx` on `PATH`.
fn agent_binary() -> std::ffi::OsString {
    let name = if cfg!(windows) { "aonyx.exe" } else { "aonyx" };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(name);
            if candidate.exists() {
                return candidate.into_os_string();
            }
        }
    }
    std::ffi::OsString::from("aonyx")
}

/// Launch a local `aonyx serve api` on a free loopback port and return its base
/// URL. The desktop's "embedded" mode: the agent ships as a Tauri sidecar
/// (`externalBin`), so nothing extra to install; falls back to `aonyx` on `PATH`.
#[tauri::command]
fn start_local(state: tauri::State<'_, LocalAgent>) -> Result<String, String> {
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "state lock poisoned".to_string())?;
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
    }
    let port = free_port();
    let mut cmd = std::process::Command::new(agent_binary());
    cmd.args([
        "serve",
        "api",
        "--port",
        &port.to_string(),
        "--bind",
        "127.0.0.1",
    ]);
    // Don't pop a console window for the child on Windows.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let child = cmd.spawn().map_err(|e| {
        format!(
            "could not launch the bundled `aonyx` agent (sidecar) nor one on PATH \
             (build with `--features api,rag`): {e}"
        )
    })?;
    *guard = Some(child);
    Ok(format!("http://127.0.0.1:{port}"))
}

/// Stop the managed local agent, if any.
#[tauri::command]
fn stop_local(state: tauri::State<'_, LocalAgent>) {
    if let Ok(mut guard) = state.0.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
        }
    }
}

/// Parse an indicatif/hf-hub progress fragment like "12.34 MiB/80.00 MiB" into
/// (downloaded_bytes, total_bytes). Returns None when no size pair is present.
fn parse_progress(s: &str) -> Option<(u64, u64)> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"([\d.]+)\s*([KMGT]?i?B)\s*/\s*([\d.]+)\s*([KMGT]?i?B)").unwrap()
    });
    let c = re.captures(s)?;
    let unit = |u: &str| -> f64 {
        match u {
            "KiB" | "KB" => 1024.0,
            "MiB" | "MB" => 1024.0 * 1024.0,
            "GiB" | "GB" => 1024.0 * 1024.0 * 1024.0,
            "TiB" | "TB" => 1024.0_f64.powi(4),
            _ => 1.0,
        }
    };
    let dn: f64 = c.get(1)?.as_str().parse().ok()?;
    let tn: f64 = c.get(3)?.as_str().parse().ok()?;
    let d = (dn * unit(c.get(2)?.as_str())) as u64;
    let t = (tn * unit(c.get(4)?.as_str())) as u64;
    if t > 0 {
        Some((d, t))
    } else {
        None
    }
}

/// Download the local embedding model via the bundled agent, relaying fastembed's
/// stderr download progress to the frontend as structured events (ADR-016 / W4):
/// `{phase:"downloading", downloaded, total, pct}`, then `{phase:"done"}` (or
/// `{phase:"error"}`). Returns quickly when the model is already cached.
#[tauri::command]
async fn prepare_embeddings(on_event: tauri::ipc::Channel<Value>) -> Result<(), String> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(agent_binary());
    cmd.args(["memory", "prepare-embeddings"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not launch the bundled agent: {e}"))?;
    let mut stderr = child.stderr.take().ok_or("no stderr from the agent")?;

    // fastembed's hf-hub bar is carriage-return updated on stderr. Read raw
    // bytes (split on \r and \n), emit each "X/Y" size pair as progress, then
    // wait for the child — all on a blocking task so the runtime stays free.
    let ev = on_event.clone();
    let status = tauri::async_runtime::spawn_blocking(move || {
        let mut chunk = [0u8; 4096];
        let mut acc: Vec<u8> = Vec::new();
        while let Ok(n) = stderr.read(&mut chunk) {
            if n == 0 {
                break;
            }
            for &b in &chunk[..n] {
                if b == b'\r' || b == b'\n' {
                    if !acc.is_empty() {
                        if let Some((d, t)) = parse_progress(&String::from_utf8_lossy(&acc)) {
                            let pct = (d as f64 / t as f64 * 100.0).round() as u64;
                            let _ = ev.send(serde_json::json!({
                                "phase": "downloading", "downloaded": d, "total": t, "pct": pct
                            }));
                        }
                        acc.clear();
                    }
                } else {
                    acc.push(b);
                }
            }
        }
        child.wait()
    })
    .await
    .map_err(|e| format!("join: {e}"))?
    .map_err(|e| format!("agent wait: {e}"))?;

    if status.success() {
        let _ = on_event.send(serde_json::json!({ "phase": "done" }));
        Ok(())
    } else {
        let _ = on_event.send(
            serde_json::json!({ "phase": "error", "message": "prepare-embeddings failed" }),
        );
        Err(format!("prepare-embeddings exited with status {status}"))
    }
}

/// Path to the agent's global config (`~/.aonyx/config.toml`).
fn aonyx_config_path() -> Result<std::path::PathBuf, String> {
    dirs::home_dir()
        .map(|h| h.join(".aonyx").join("config.toml"))
        .ok_or_else(|| "could not resolve home directory".to_string())
}

/// Read the provider-relevant fields from `~/.aonyx/config.toml` (defaults
/// when the file is absent) so the wizard can pre-fill.
#[tauri::command]
fn read_provider_config() -> Result<Value, String> {
    let path = aonyx_config_path()?;
    let table: toml::value::Table = if path.exists() {
        toml::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("parse config: {e}"))?
    } else {
        toml::value::Table::new()
    };
    let s = |k: &str| {
        table
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    Ok(serde_json::json!({
        "provider": table.get("provider").and_then(|v| v.as_str()).unwrap_or("anthropic"),
        "model": s("model"),
        "anthropic_api_key": s("anthropic_api_key"),
        "openai_api_key": s("openai_api_key"),
        "openrouter_api_key": s("openrouter_api_key"),
        "openai_base_url": s("openai_base_url"),
        "ollama_base_url": s("ollama_base_url"),
        "lm_studio_base_url": s("lm_studio_base_url"),
        "claude_code_binary": s("claude_code_binary"),
    }))
}

/// Merge the wizard's provider fields into `~/.aonyx/config.toml`, preserving
/// every other key (mcp_servers, tools_allow, …). Optional values are set
/// when non-empty and removed when explicitly blanked.
#[tauri::command]
fn save_provider_config(cfg: Value) -> Result<(), String> {
    let path = aonyx_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    // Format-preserving edit: keeps existing keys + tables ([[mcp_servers]],
    // [custom_theme], tools_allow, …) intact, no risky key reordering.
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("parse config: {e}"))?;

    if let Some(p) = cfg.get("provider").and_then(|v| v.as_str()) {
        doc["provider"] = toml_edit::value(p);
    }
    if let Some(m) = cfg
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|m| !m.is_empty())
    {
        doc["model"] = toml_edit::value(m);
    }
    for key in [
        "anthropic_api_key",
        "openai_api_key",
        "openrouter_api_key",
        "openai_base_url",
        "ollama_base_url",
        "lm_studio_base_url",
        "claude_code_binary",
    ] {
        match cfg.get(key).and_then(|v| v.as_str()) {
            Some(v) if !v.is_empty() => {
                doc[key] = toml_edit::value(v);
            }
            Some(_) => {
                doc.as_table_mut().remove(key);
            }
            None => {}
        }
    }

    std::fs::write(&path, doc.to_string()).map_err(|e| e.to_string())
}

/// Read whether first-run setup has completed: a `setup_complete = true` marker
/// in `~/.aonyx/config.toml` together with a non-empty `provider` + `model`. The
/// desktop gates the first-run wizard on this (ADR-016).
#[tauri::command]
fn setup_state() -> Result<Value, String> {
    let path = aonyx_config_path()?;
    if !path.exists() {
        return Ok(serde_json::json!({ "configured": false }));
    }
    let table: toml::value::Table =
        toml::from_str(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("parse config: {e}"))?;
    let non_empty = |k: &str| {
        table
            .get(k)
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    };
    let complete = table
        .get("setup_complete")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok(serde_json::json!({
        "configured": complete && non_empty("provider") && non_empty("model")
    }))
}

/// Persist the wizard's full choice set into `~/.aonyx/config.toml`: the provider
/// fields (as `save_provider_config`) plus the RAG backend + embeddings under
/// `[rag]` (ADR-008/009), and flips `setup_complete = true`. Format-preserving.
#[tauri::command]
fn save_setup(cfg: Value) -> Result<(), String> {
    let path = aonyx_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("parse config: {e}"))?;

    if let Some(p) = cfg.get("provider").and_then(|v| v.as_str()) {
        doc["provider"] = toml_edit::value(p);
    }
    if let Some(m) = cfg
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|m| !m.is_empty())
    {
        doc["model"] = toml_edit::value(m);
    }
    for key in [
        "anthropic_api_key",
        "openai_api_key",
        "openrouter_api_key",
        "openai_base_url",
        "ollama_base_url",
        "lm_studio_base_url",
        "claude_code_binary",
    ] {
        match cfg.get(key).and_then(|v| v.as_str()) {
            Some(v) if !v.is_empty() => {
                doc[key] = toml_edit::value(v);
            }
            Some(_) => {
                doc.as_table_mut().remove(key);
            }
            None => {}
        }
    }

    let backend = cfg
        .get("rag_backend")
        .and_then(|v| v.as_str())
        .unwrap_or("local");
    let embeddings = cfg
        .get("rag_embeddings")
        .and_then(|v| v.as_str())
        .unwrap_or("local");
    if !doc.as_table().contains_key("rag") {
        doc["rag"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    doc["rag"]["backend"] = toml_edit::value(backend);
    doc["rag"]["embeddings"] = toml_edit::value(embeddings);

    doc["setup_complete"] = toml_edit::value(true);

    std::fs::write(&path, doc.to_string()).map_err(|e| e.to_string())
}

// ─── Custom sub-agents (ADR-017 / MA2): CRUD over ~/.aonyx/agents/*.AGENT.md ──

/// `~/.aonyx/agents`.
fn aonyx_agents_dir() -> Result<std::path::PathBuf, String> {
    dirs::home_dir()
        .map(|h| h.join(".aonyx").join("agents"))
        .ok_or_else(|| "could not resolve home directory".to_string())
}

fn slugify(s: &str) -> String {
    let raw: String = s
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    raw.trim_matches('-').to_string()
}

/// The frontmatter of an `AGENT.md` file (the markdown body is handled
/// separately). Mirrors `aonyx_agent::agents::AgentDefinition`'s file fields.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct AgentFile {
    #[serde(default)]
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default)]
    tools: Vec<String>,
}

/// Split `---\n…\n---\n<body>` → `(frontmatter, body)` (same as the agent loader).
fn split_agent_md(raw: &str) -> Option<(&str, &str)> {
    let after = raw.strip_prefix("---\r\n").or_else(|| raw.strip_prefix("---\n"))?;
    let mut cur = 0;
    while let Some(rel) = after[cur..].find("\n---") {
        let abs = cur + rel;
        let tail = &after[abs + 4..];
        if tail.is_empty() || tail.starts_with('\n') || tail.starts_with('\r') {
            return Some((&after[..abs], tail.trim_start_matches(['\r', '\n'])));
        }
        cur = abs + 4;
    }
    None
}

/// List the user-defined sub-agents in `~/.aonyx/agents/`. Built-in presets
/// (coder/reviewer/researcher) live in the agent binary and are always
/// available; this only manages the editable user files.
#[tauri::command]
fn agents_list() -> Result<Value, String> {
    let dir = aonyx_agents_dir()?;
    let mut agents = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let path = e.path();
            let lower = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if lower != "agent.md" && !lower.ends_with(".agent.md") {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some((fm, body)) = split_agent_md(&raw) else {
                continue;
            };
            let Ok(a) = serde_yaml::from_str::<AgentFile>(fm) else {
                continue;
            };
            if let Ok(mut v) = serde_json::to_value(&a) {
                if let Some(o) = v.as_object_mut() {
                    o.insert("body".into(), Value::String(body.to_string()));
                    o.insert("file".into(), Value::String(path.display().to_string()));
                }
                agents.push(v);
            }
        }
    }
    Ok(serde_json::json!({ "dir": dir.display().to_string(), "agents": agents }))
}

/// Write a sub-agent to `~/.aonyx/agents/<id>.AGENT.md` (creates the dir).
#[tauri::command]
fn agents_save(agent: Value) -> Result<(), String> {
    let dir = aonyx_agents_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut meta: AgentFile =
        serde_json::from_value(agent.clone()).map_err(|e| format!("bad agent: {e}"))?;
    if meta.name.trim().is_empty() {
        return Err("the agent needs a name".to_string());
    }
    if meta.id.trim().is_empty() {
        meta.id = slugify(&meta.name);
    }
    if meta.id.is_empty() {
        return Err("the agent needs a valid id".to_string());
    }
    meta.tools.retain(|t| !t.trim().is_empty());
    let body = agent.get("body").and_then(|v| v.as_str()).unwrap_or("").trim();
    let fm = serde_yaml::to_string(&meta).map_err(|e| e.to_string())?;
    let content = format!("---\n{fm}---\n\n{body}\n");
    std::fs::write(dir.join(format!("{}.AGENT.md", meta.id)), content).map_err(|e| e.to_string())
}

/// Delete a sub-agent file by id.
#[tauri::command]
fn agents_delete(id: String) -> Result<(), String> {
    let safe = slugify(&id);
    if safe.is_empty() {
        return Err("bad agent id".to_string());
    }
    let path = aonyx_agents_dir()?.join(format!("{safe}.AGENT.md"));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Check the configured update endpoint. Returns the new version's metadata
/// when an update is available, or `null` when the app is already current.
/// Runs entirely Rust-side (the webview only calls this command), so no
/// updater JS-capability is needed.
#[tauri::command]
async fn check_for_update(app: tauri::AppHandle) -> Result<Option<Value>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await.map_err(|e| e.to_string())? {
        Some(u) => Ok(Some(serde_json::json!({
            "version": u.version,
            "currentVersion": u.current_version,
            "notes": u.body,
            "date": u.date.map(|d| d.to_string()),
        }))),
        None => Ok(None),
    }
}

/// Download + install the pending update (verifying its minisign signature
/// against the bundled pubkey), then relaunch. Errors when nothing is
/// available.
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no update available".to_string())?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    // `restart()` diverges (`-> !`): the process re-execs into the new build.
    app.restart();
}

// ─── aonyx-account: device-code grant + keyring token storage (ADR-011) ──────
const ACCOUNT_SERVICE: &str = "aonyx-agent";

fn account_entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(ACCOUNT_SERVICE, key).map_err(|e| e.to_string())
}

/// Stable per-install device id, kept in the OS keyring.
fn device_id() -> String {
    if let Ok(entry) = account_entry("device-id") {
        if let Ok(id) = entry.get_password() {
            if !id.is_empty() {
                return id;
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        let _ = entry.set_password(&id);
        return id;
    }
    uuid::Uuid::new_v4().to_string()
}

/// Open a URL in the user's default system browser.
fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

fn account_base(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    if b.is_empty() {
        "https://account.aonyx.fr".to_string()
    } else {
        b.to_string()
    }
}

fn account_access_token() -> Option<String> {
    let raw = account_entry("tokens").ok()?.get_password().ok()?;
    let json: Value = serde_json::from_str(&raw).ok()?;
    json.get("accessToken")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Start the device-code grant: request a code, then open the browser to approve.
#[tauri::command]
async fn account_device_start(base: String) -> Result<Value, String> {
    let url = format!("{}/api/v1/auth/device/code", account_base(&base));
    let body = serde_json::json!({ "product": "aonyx-agent", "deviceId": device_id() });
    let resp = reqwest::Client::new()
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| format!("bad JSON: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {json}"));
    }
    if let Some(v) = json.get("verificationUrl").and_then(|v| v.as_str()) {
        open_browser(v);
    }
    Ok(json)
}

/// Poll once for the device token. The body carries `status`
/// (pending / approved / denied / expired) plus `tokens` + `user` when approved.
#[tauri::command]
async fn account_device_poll(base: String, device_code: String) -> Result<Value, String> {
    let url = format!("{}/api/v1/auth/device/token", account_base(&base));
    let body = serde_json::json!({ "deviceCode": device_code });
    let resp = reqwest::Client::new()
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    resp.json().await.map_err(|e| format!("bad JSON: {e}"))
}

/// Persist access + refresh tokens in the OS keyring.
#[tauri::command]
fn account_store(access: String, refresh: String) -> Result<(), String> {
    let val = serde_json::json!({ "accessToken": access, "refreshToken": refresh }).to_string();
    account_entry("tokens")?
        .set_password(&val)
        .map_err(|e| e.to_string())
}

/// Clear stored tokens (sign out).
#[tauri::command]
fn account_logout() -> Result<(), String> {
    if let Ok(e) = account_entry("tokens") {
        let _ = e.set_password("{}");
    }
    Ok(())
}

/// True if an access token is stored.
#[tauri::command]
fn account_has_token() -> bool {
    account_access_token().is_some()
}

/// Fetch the signed-in user's profile (Bearer the stored access token).
#[tauri::command]
async fn account_me(base: String) -> Result<Value, String> {
    let token = account_access_token().ok_or_else(|| "NOT_AUTHENTICATED".to_string())?;
    let url = format!("{}/api/v1/auth/profile", account_base(&base));
    let resp = reqwest::Client::new()
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| format!("bad JSON: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    Ok(json)
}

/// Run the desktop application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;
    tauri::Builder::default()
        .manage(LocalAgent::default())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            api_info,
            api_create_session,
            api_approve,
            api_send,
            api_stream,
            api_list_sessions,
            api_get_session,
            api_memory_search,
            api_kg_entities,
            api_kg_relations,
            api_tools,
            start_local,
            stop_local,
            prepare_embeddings,
            read_provider_config,
            save_provider_config,
            save_setup,
            setup_state,
            agents_list,
            agents_save,
            agents_delete,
            list_models,
            claude_login,
            check_for_update,
            install_update,
            account_device_start,
            account_device_poll,
            account_store,
            account_logout,
            account_has_token,
            account_me
        ])
        .on_window_event(|window, event| {
            // Kill the managed local agent when the window closes so it never
            // orphans.
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if let Some(state) = window.try_state::<LocalAgent>() {
                    if let Ok(mut guard) = state.0.lock() {
                        if let Some(mut child) = guard.take() {
                            let _ = child.kill();
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running the Aonyx desktop app");
}

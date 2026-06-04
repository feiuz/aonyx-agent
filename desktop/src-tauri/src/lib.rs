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
            // Real live fetch: reuse Claude Code's own OAuth session token
            // (~/.claude/.credentials.json) against the Anthropic Models API,
            // else fall back to ANTHROPIC_API_KEY. No hardcoded list.
            if let Some(token) = read_claude_code_token() {
                client
                    .get("https://api.anthropic.com/v1/models?limit=1000")
                    .header("authorization", format!("Bearer {token}"))
                    .header("anthropic-version", "2023-06-01")
            } else if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
                client
                    .get("https://api.anthropic.com/v1/models?limit=1000")
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01")
            } else {
                return Err(
                    "Claude Code non connecté — lance `claude` puis /login (ou définis ANTHROPIC_API_KEY)."
                        .to_string(),
                );
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

/// Read Claude Code's stored OAuth access token (`~/.claude/.credentials.json`,
/// key `claudeAiOauth.accessToken`) so models can be listed from the Anthropic
/// Models API using the user's existing Claude Code session — no separate key.
fn read_claude_code_token() -> Option<String> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let path = std::path::Path::new(&home)
        .join(".claude")
        .join(".credentials.json");
    let data = std::fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&data).ok()?;
    json.get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .map(String::from)
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

/// Launch a local `aonyx serve api` on a free loopback port and return its
/// base URL. The desktop's "embedded" mode: no separate server to start.
/// Requires `aonyx` (built with `--features api`) on the PATH.
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
    let mut cmd = std::process::Command::new("aonyx");
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
            "could not launch `aonyx serve api` — is `aonyx` on your PATH \
             (built with `--features api`)? {e}"
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
            api_send,
            api_stream,
            api_list_sessions,
            api_get_session,
            api_memory_search,
            start_local,
            stop_local,
            read_provider_config,
            save_provider_config,
            list_models,
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

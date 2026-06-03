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
    let resp = rb.send().await.map_err(|e| format!("request failed: {e}"))?;
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
    send(reqwest::Method::POST, join(&base, &path), &token, Some(body)).await
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
    let mut rb = client.post(&url).json(&serde_json::json!({ "content": content }));
    if !token.is_empty() {
        rb = rb.bearer_auth(token);
    }
    let resp = rb.send().await.map_err(|e| format!("request failed: {e}"))?;
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
    finish(rb.send().await.map_err(|e| format!("request failed: {e}"))?).await
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
    finish(rb.send().await.map_err(|e| format!("request failed: {e}"))?).await
}

/// Run the desktop application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            api_info,
            api_create_session,
            api_send,
            api_stream,
            api_list_sessions,
            api_get_session,
            api_memory_search
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Aonyx desktop app");
}

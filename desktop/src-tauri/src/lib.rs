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

/// Run the desktop application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            api_info,
            api_create_session,
            api_send
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Aonyx desktop app");
}

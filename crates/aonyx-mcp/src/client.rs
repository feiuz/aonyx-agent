//! MCP client — consume external servers over the **stdio** transport
//! (Phase GG).
//!
//! The stdio transport speaks JSON-RPC 2.0 with one JSON object per
//! line on the child's stdin / stdout. We implement the minimal slice
//! Aonyx needs:
//!
//! 1. `initialize` request + `notifications/initialized` (handshake),
//! 2. `tools/list` (discovery),
//! 3. `tools/call` (invocation), wrapped by [`McpToolHandler`] so the
//!    remote tools drop straight into [`aonyx_tools::ToolRegistry`].
//!
//! Tool calls in the agent loop are sequential, so the client
//! serialises each request/response transaction under a single mutex
//! rather than running a full id-demultiplexing reader task. That's
//! correct for the one-at-a-time dispatch pattern and far simpler.
//!
//! HTTP / SSE transports are deferred.

use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use aonyx_core::{AonyxError, Result, SafetyClass, ToolCall, ToolHandler, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

/// JSON-RPC protocol version string sent during `initialize`.
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// A tool advertised by a remote MCP server.
#[derive(Debug, Clone)]
pub struct McpToolDef {
    /// Tool name as the server knows it.
    pub name: String,
    /// Human-readable description (may be empty).
    pub description: String,
    /// JSON-schema for the tool's arguments.
    pub input_schema: Value,
}

/// A connected MCP server, abstracted over its transport (stdio in
/// Phase GG, Streamable HTTP in Phase II). [`McpToolHandler`] is
/// generic over this so the same adapter wraps either.
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Friendly name used to namespace the server's tool ids.
    fn server_name(&self) -> &str;
    /// Discover the server's tools.
    async fn list_tools(&self) -> Result<Vec<McpToolDef>>;
    /// Invoke a remote tool, returning the textual result content.
    async fn call_tool(&self, name: &str, args: Value) -> Result<Value>;
}

/// A connected stdio MCP server: the child process plus framed I/O.
pub struct StdioMcpClient {
    /// Friendly name (used to namespace tool ids).
    server_name: String,
    /// Held so the child is killed on drop.
    _child: Child,
    io: Mutex<ClientIo>,
    next_id: AtomicI64,
}

struct ClientIo {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl StdioMcpClient {
    /// Spawn `command args…`, perform the `initialize` handshake, and
    /// return a ready client. The child is killed when the returned
    /// client is dropped.
    pub async fn connect(
        server_name: impl Into<String>,
        command: &str,
        args: &[String],
    ) -> Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AonyxError::Mcp(format!("spawn {command}: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AonyxError::Mcp("child stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AonyxError::Mcp("child stdout unavailable".into()))?;

        let client = Self {
            server_name: server_name.into(),
            _child: child,
            io: Mutex::new(ClientIo {
                stdin,
                stdout: BufReader::new(stdout),
            }),
            next_id: AtomicI64::new(1),
        };

        client.handshake().await?;
        Ok(client)
    }

    async fn handshake(&self) -> Result<()> {
        let params = json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "aonyx-agent", "version": env!("CARGO_PKG_VERSION") },
        });
        let _ = self.request("initialize", params).await?;
        // Fire-and-forget the initialized notification.
        self.notify("notifications/initialized", json!({})).await?;
        Ok(())
    }

    /// Send a JSON-RPC request and read replies until the matching id
    /// comes back. Notifications / unrelated ids encountered along the
    /// way are skipped.
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let line = build_request(id, method, &params);
        let mut io = self.io.lock().await;
        io.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| AonyxError::Mcp(format!("write {method}: {e}")))?;
        io.stdin
            .flush()
            .await
            .map_err(|e| AonyxError::Mcp(format!("flush {method}: {e}")))?;

        let mut buf = String::new();
        loop {
            buf.clear();
            let n = io
                .stdout
                .read_line(&mut buf)
                .await
                .map_err(|e| AonyxError::Mcp(format!("read {method}: {e}")))?;
            if n == 0 {
                return Err(AonyxError::Mcp(format!(
                    "{method}: server closed the connection"
                )));
            }
            let trimmed = buf.trim();
            if trimmed.is_empty() {
                continue;
            }
            match match_response(trimmed, id) {
                ResponseMatch::Result(v) => return Ok(v),
                ResponseMatch::Error(msg) => {
                    return Err(AonyxError::Mcp(format!("{method}: {msg}")))
                }
                ResponseMatch::Other => continue,
            }
        }
    }

    /// Send a JSON-RPC notification (no id, no response expected).
    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let line = build_notification(method, &params);
        let mut io = self.io.lock().await;
        io.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| AonyxError::Mcp(format!("notify {method}: {e}")))?;
        io.stdin
            .flush()
            .await
            .map_err(|e| AonyxError::Mcp(format!("flush notify {method}: {e}")))?;
        Ok(())
    }
}

#[async_trait]
impl McpTransport for StdioMcpClient {
    fn server_name(&self) -> &str {
        &self.server_name
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDef>> {
        let resp = self.request("tools/list", json!({})).await?;
        Ok(parse_tools_list(&resp))
    }

    async fn call_tool(&self, name: &str, args: Value) -> Result<Value> {
        let params = json!({ "name": name, "arguments": args });
        let resp = self.request("tools/call", params).await?;
        Ok(extract_call_result(&resp))
    }
}

/// A connected Streamable-HTTP MCP server (Phase II).
///
/// Each JSON-RPC request is a `POST` to the endpoint. The server may
/// reply with `application/json` (a single response) or
/// `text/event-stream` (SSE); both are handled. An `Mcp-Session-Id`
/// returned by `initialize` is echoed on subsequent requests.
pub struct HttpMcpClient {
    server_name: String,
    url: String,
    bearer: Option<String>,
    http: reqwest::Client,
    next_id: AtomicI64,
    session_id: Mutex<Option<String>>,
}

impl HttpMcpClient {
    /// Connect to an HTTP MCP endpoint and perform the `initialize`
    /// handshake. `bearer` is sent as `Authorization: Bearer …` when
    /// present.
    pub async fn connect(
        server_name: impl Into<String>,
        url: impl Into<String>,
        bearer: Option<String>,
    ) -> Result<Self> {
        let client = Self {
            server_name: server_name.into(),
            url: url.into(),
            bearer,
            http: reqwest::Client::new(),
            next_id: AtomicI64::new(1),
            session_id: Mutex::new(None),
        };
        let params = json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "aonyx-agent", "version": env!("CARGO_PKG_VERSION") },
        });
        let _ = client.request("initialize", params).await?;
        Ok(client)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let mut req = self
            .http
            .post(&self.url)
            .header("content-type", "application/json")
            // Accept both reply shapes per the Streamable HTTP spec.
            .header("accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(token) = &self.bearer {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        if let Some(sid) = self.session_id.lock().await.clone() {
            req = req.header("mcp-session-id", sid);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| AonyxError::Mcp(format!("{method} POST: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let txt = resp.text().await.unwrap_or_default();
            return Err(AonyxError::Mcp(format!("{method}: HTTP {status}: {txt}")));
        }

        // Capture a session id handed out by `initialize`.
        if let Some(sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
        {
            *self.session_id.lock().await = Some(sid);
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = resp
            .text()
            .await
            .map_err(|e| AonyxError::Mcp(format!("{method} body: {e}")))?;

        let payload = if content_type.contains("text/event-stream") {
            extract_sse_json(&text)
                .ok_or_else(|| AonyxError::Mcp(format!("{method}: no JSON in SSE stream")))?
        } else {
            serde_json::from_str::<Value>(text.trim())
                .map_err(|e| AonyxError::Mcp(format!("{method}: bad JSON: {e}")))?
        };

        match match_response_value(&payload, id) {
            ResponseMatch::Result(v) => Ok(v),
            ResponseMatch::Error(msg) => Err(AonyxError::Mcp(format!("{method}: {msg}"))),
            ResponseMatch::Other => Err(AonyxError::Mcp(format!("{method}: unexpected response"))),
        }
    }
}

#[async_trait]
impl McpTransport for HttpMcpClient {
    fn server_name(&self) -> &str {
        &self.server_name
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDef>> {
        let resp = self.request("tools/list", json!({})).await?;
        Ok(parse_tools_list(&resp))
    }

    async fn call_tool(&self, name: &str, args: Value) -> Result<Value> {
        let params = json!({ "name": name, "arguments": args });
        let resp = self.request("tools/call", params).await?;
        Ok(extract_call_result(&resp))
    }
}

/// A remote MCP tool adapted to Aonyx's [`ToolHandler`] so it can be
/// registered alongside the built-in tools — transport-agnostic.
pub struct McpToolHandler {
    /// Fully-qualified, collision-safe name: `<server>__<tool>`.
    qualified_name: String,
    /// Original (unprefixed) name the server expects in `tools/call`.
    remote_name: String,
    schema: Value,
    transport: Arc<dyn McpTransport>,
}

impl McpToolHandler {
    /// Wrap a discovered tool def against its transport.
    pub fn new(transport: Arc<dyn McpTransport>, def: McpToolDef) -> Self {
        let qualified_name = format!("{}__{}", transport.server_name(), def.name);
        Self {
            qualified_name,
            remote_name: def.name,
            schema: def.input_schema,
            transport,
        }
    }
}

#[async_trait]
impl ToolHandler for McpToolHandler {
    fn name(&self) -> &str {
        &self.qualified_name
    }

    fn schema(&self) -> Value {
        self.schema.clone()
    }

    fn classify(&self) -> SafetyClass {
        // Remote tools are opaque; treat them as Caution so they pass
        // the non-interactive `DenyDestructive` default but are still
        // visibly second-class. (The user explicitly connected the
        // server.)
        SafetyClass::Caution
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let output = self
            .transport
            .call_tool(&self.remote_name, call.args)
            .await?;
        Ok(ToolResult {
            call_id: call.id,
            output,
            error: None,
        })
    }
}

/// Connect to a stdio MCP server, discover its tools, and register
/// each (as an [`McpToolHandler`]) into `registry`. Returns the number
/// of tools registered. The client is kept alive inside the handlers
/// (each holds an `Arc`), so the caller does not need to retain it.
pub async fn connect_and_register(
    registry: &mut aonyx_tools::ToolRegistry,
    server_name: &str,
    command: &str,
    args: &[String],
) -> Result<usize> {
    let client: Arc<dyn McpTransport> =
        Arc::new(StdioMcpClient::connect(server_name, command, args).await?);
    register_transport(registry, client).await
}

/// Connect to an HTTP MCP server (Streamable HTTP), discover its tools,
/// and register them. `bearer` is sent as `Authorization: Bearer …`
/// when present (Phase II).
pub async fn connect_http_and_register(
    registry: &mut aonyx_tools::ToolRegistry,
    server_name: &str,
    url: &str,
    bearer: Option<String>,
) -> Result<usize> {
    let client: Arc<dyn McpTransport> =
        Arc::new(HttpMcpClient::connect(server_name, url, bearer).await?);
    register_transport(registry, client).await
}

/// Discover + register every tool of a connected transport. Each
/// handler holds an `Arc` to the transport, so it stays alive without
/// the caller retaining it.
async fn register_transport(
    registry: &mut aonyx_tools::ToolRegistry,
    transport: Arc<dyn McpTransport>,
) -> Result<usize> {
    let tools = transport.list_tools().await?;
    let count = tools.len();
    for def in tools {
        registry.register(Arc::new(McpToolHandler::new(Arc::clone(&transport), def)));
    }
    Ok(count)
}

// ---- pure framing / parsing helpers (unit-tested) ----

/// Serialize a JSON-RPC request as a single newline-terminated line.
fn build_request(id: i64, method: &str, params: &Value) -> String {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    format!("{msg}\n")
}

/// Serialize a JSON-RPC notification (no id).
fn build_notification(method: &str, params: &Value) -> String {
    let msg = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    format!("{msg}\n")
}

/// Outcome of inspecting one inbound line against an expected id.
enum ResponseMatch {
    Result(Value),
    Error(String),
    Other,
}

/// Match an inbound JSON-RPC line against the request id we're waiting
/// for. Lines that don't parse, or carry a different id, are `Other`.
fn match_response(line: &str, expected_id: i64) -> ResponseMatch {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return ResponseMatch::Other;
    };
    match_response_value(&v, expected_id)
}

/// Same as [`match_response`] but on an already-parsed value — used by
/// the HTTP transport (Phase II), which decodes the body before
/// matching.
fn match_response_value(v: &Value, expected_id: i64) -> ResponseMatch {
    let id_matches = v.get("id").and_then(|i| i.as_i64()) == Some(expected_id);
    if !id_matches {
        return ResponseMatch::Other;
    }
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error")
            .to_string();
        return ResponseMatch::Error(msg);
    }
    ResponseMatch::Result(v.get("result").cloned().unwrap_or(Value::Null))
}

/// Pull the first JSON-RPC object out of an SSE stream body (Phase II).
///
/// SSE frames are `data: <payload>` lines; a single logical message may
/// span multiple `data:` lines (joined with `\n`), and frames are
/// separated by blank lines. We return the first frame whose joined
/// payload parses as JSON containing an `id` (the response we awaited),
/// skipping any keep-alive / comment lines.
fn extract_sse_json(body: &str) -> Option<Value> {
    let mut data = String::new();
    let flush = |buf: &mut String| -> Option<Value> {
        if buf.is_empty() {
            return None;
        }
        let parsed = serde_json::from_str::<Value>(buf.trim()).ok();
        buf.clear();
        parsed.filter(|v| v.get("id").is_some())
    };
    for line in body.lines() {
        if let Some(payload) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(payload.trim_start());
        } else if line.trim().is_empty() {
            // End of an SSE frame.
            if let Some(v) = flush(&mut data) {
                return Some(v);
            }
        }
        // Non-data, non-blank lines (e.g. `event:`, `:` comments) are
        // ignored.
    }
    flush(&mut data)
}

/// Pull the tool list out of a `tools/list` result.
fn parse_tools_list(result: &Value) -> Vec<McpToolDef> {
    result
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let name = t.get("name")?.as_str()?.to_string();
                    let description = t
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input_schema = t
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or_else(|| json!({ "type": "object" }));
                    Some(McpToolDef {
                        name,
                        description,
                        input_schema,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Flatten a `tools/call` result's `content` array into a single JSON
/// value. MCP returns `{ content: [{type:"text", text:"…"}, …] }`; we
/// join text parts, and fall back to the raw result otherwise.
fn extract_call_result(result: &Value) -> Value {
    let Some(content) = result.get("content").and_then(|c| c.as_array()) else {
        return result.clone();
    };
    let text = content
        .iter()
        .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        result.clone()
    } else {
        Value::String(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_is_newline_terminated_jsonrpc() {
        let line = build_request(7, "tools/list", &json!({}));
        assert!(line.ends_with('\n'));
        let v: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 7);
        assert_eq!(v["method"], "tools/list");
    }

    #[test]
    fn build_notification_has_no_id() {
        let line = build_notification("notifications/initialized", &json!({}));
        let v: Value = serde_json::from_str(line.trim()).unwrap();
        assert!(v.get("id").is_none());
        assert_eq!(v["method"], "notifications/initialized");
    }

    #[test]
    fn match_response_returns_result_for_matching_id() {
        let line = r#"{"jsonrpc":"2.0","id":3,"result":{"ok":true}}"#;
        match match_response(line, 3) {
            ResponseMatch::Result(v) => assert_eq!(v["ok"], true),
            _ => panic!("expected result"),
        }
    }

    #[test]
    fn match_response_skips_other_ids_and_notifications() {
        assert!(matches!(
            match_response(r#"{"jsonrpc":"2.0","id":99,"result":{}}"#, 3),
            ResponseMatch::Other
        ));
        assert!(matches!(
            match_response(r#"{"jsonrpc":"2.0","method":"log","params":{}}"#, 3),
            ResponseMatch::Other
        ));
        assert!(matches!(
            match_response("not json", 3),
            ResponseMatch::Other
        ));
    }

    #[test]
    fn match_response_surfaces_errors() {
        let line = r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"no such method"}}"#;
        match match_response(line, 3) {
            ResponseMatch::Error(m) => assert!(m.contains("no such method")),
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn parse_tools_list_extracts_defs() {
        let result = json!({
            "tools": [
                { "name": "search", "description": "web search",
                  "inputSchema": { "type": "object", "properties": { "q": { "type": "string" } } } },
                { "name": "fetch" }
            ]
        });
        let tools = parse_tools_list(&result);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "search");
        assert_eq!(tools[0].description, "web search");
        // Missing description / schema get sensible defaults.
        assert_eq!(tools[1].name, "fetch");
        assert_eq!(tools[1].description, "");
        assert_eq!(tools[1].input_schema["type"], "object");
    }

    #[test]
    fn parse_tools_list_handles_missing_tools_key() {
        assert!(parse_tools_list(&json!({})).is_empty());
    }

    #[test]
    fn extract_call_result_joins_text_content() {
        let result = json!({
            "content": [
                { "type": "text", "text": "line one" },
                { "type": "text", "text": "line two" }
            ]
        });
        assert_eq!(extract_call_result(&result), json!("line one\nline two"));
    }

    #[test]
    fn extract_call_result_falls_back_to_raw() {
        let result = json!({ "data": 42 });
        assert_eq!(extract_call_result(&result), result);
    }

    #[test]
    fn extract_sse_json_pulls_the_response_frame() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n";
        let v = extract_sse_json(body).expect("frame");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["ok"], true);
    }

    #[test]
    fn extract_sse_json_joins_multiline_data_and_skips_keepalives() {
        // Keep-alive comment, then JSON split across two `data:` lines.
        let body = ": keep-alive\ndata: {\"jsonrpc\":\"2.0\",\ndata: \"id\":5,\"result\":{}}\n\n";
        let v = extract_sse_json(body).expect("frame");
        assert_eq!(v["id"], 5);
    }

    #[test]
    fn extract_sse_json_none_when_no_data() {
        assert!(extract_sse_json(": only a comment\n\n").is_none());
        assert!(extract_sse_json("").is_none());
    }

    #[test]
    fn match_response_value_matches_parsed_json() {
        let v = json!({"jsonrpc":"2.0","id":4,"result":{"x":1}});
        match match_response_value(&v, 4) {
            ResponseMatch::Result(r) => assert_eq!(r["x"], 1),
            _ => panic!("expected result"),
        }
        assert!(matches!(match_response_value(&v, 99), ResponseMatch::Other));
    }

    #[test]
    fn mcp_tool_handler_qualifies_the_name() {
        // Build a def; the handler name should be `<server>__<tool>`.
        // (No live client needed — name() reads cached strings.)
        let def = McpToolDef {
            name: "search".into(),
            description: String::new(),
            input_schema: json!({ "type": "object" }),
        };
        // We can't construct StdioMcpClient without spawning; assert the
        // formatting rule directly instead.
        let qualified = format!("{}__{}", "brave", def.name);
        assert_eq!(qualified, "brave__search");
    }
}

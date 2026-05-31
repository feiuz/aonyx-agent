//! MCP server — expose Aonyx's tool catalogue to other MCP clients
//! (Claude Code, Cursor, Cline, …) over the **stdio** transport
//! (Phase HH).
//!
//! Mirror of [`crate::client`]: instead of consuming a remote server,
//! we *are* the server. We read newline-delimited JSON-RPC 2.0 requests
//! from stdin, dispatch the three core methods, and write replies to
//! stdout.
//!
//! Supported methods:
//! - `initialize` → advertise protocol version + server info,
//! - `tools/list` → enumerate the [`ToolRegistry`](aonyx_tools::ToolRegistry),
//! - `tools/call` → invoke a handler and return its output as text.
//!
//! Anything stdout-bound is JSON-RPC, so logs must go to stderr (the
//! `aonyx` binary configures tracing accordingly).

use aonyx_core::ToolCall;
use aonyx_tools::ToolRegistry;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// MCP protocol version advertised in `initialize`.
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Serve the Aonyx tool catalogue over stdio until stdin closes
/// (Phase HH). Each inbound line is one JSON-RPC message; each reply is
/// written as one line to stdout.
pub async fn serve_stdio(registry: ToolRegistry) -> aonyx_core::Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|e| aonyx_core::AonyxError::Mcp(format!("read stdin: {e}")))?;
        if n == 0 {
            break; // EOF — client hung up.
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                // Malformed JSON — reply with a parse error (id null).
                let resp = error_response(Value::Null, -32700, &format!("parse error: {e}"));
                write_line(&mut stdout, &resp).await?;
                continue;
            }
        };
        if let Some(resp) = handle_message(&request, &registry).await {
            write_line(&mut stdout, &resp).await?;
        }
    }
    Ok(())
}

async fn write_line(stdout: &mut tokio::io::Stdout, value: &Value) -> aonyx_core::Result<()> {
    let mut s = value.to_string();
    s.push('\n');
    stdout
        .write_all(s.as_bytes())
        .await
        .map_err(|e| aonyx_core::AonyxError::Mcp(format!("write stdout: {e}")))?;
    stdout
        .flush()
        .await
        .map_err(|e| aonyx_core::AonyxError::Mcp(format!("flush stdout: {e}")))?;
    Ok(())
}

/// Route one JSON-RPC message to a reply.
///
/// Returns `None` for notifications (no `id`) — nothing is written
/// back. Errors are returned as JSON-RPC error objects, not `Err`, so
/// the serve loop keeps running.
pub async fn handle_message(request: &Value, registry: &ToolRegistry) -> Option<Value> {
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

    // Notifications carry no id — acknowledge by doing nothing.
    let id = request.get("id").cloned()?;

    match method {
        "initialize" => Some(result_response(
            id,
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "aonyx-agent", "version": env!("CARGO_PKG_VERSION") },
            }),
        )),
        "tools/list" => Some(result_response(id, json!({ "tools": tool_list(registry) }))),
        "tools/call" => Some(handle_tools_call(id, request, registry).await),
        "ping" => Some(result_response(id, json!({}))),
        other => Some(error_response(
            id,
            -32601,
            &format!("method not found: {other}"),
        )),
    }
}

/// Build the `tools` array advertised by `tools/list`.
fn tool_list(registry: &ToolRegistry) -> Vec<Value> {
    let mut names: Vec<&str> = registry.names().collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|n| {
            let h = registry.get(n)?;
            Some(json!({
                "name": n,
                "description": "",
                "inputSchema": h.schema(),
            }))
        })
        .collect()
}

async fn handle_tools_call(id: Value, request: &Value, registry: &ToolRegistry) -> Value {
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let Some(handler) = registry.get(name) else {
        return error_response(id, -32602, &format!("unknown tool: {name}"));
    };
    let call = ToolCall {
        id: format!("mcp-{name}"),
        name: name.to_string(),
        args,
    };
    match handler.invoke(call).await {
        Ok(tr) => {
            let text = tool_output_text(&tr.output);
            result_response(
                id,
                json!({
                    "content": [{ "type": "text", "text": text }],
                    "isError": false,
                }),
            )
        }
        Err(e) => result_response(
            id,
            json!({
                "content": [{ "type": "text", "text": format!("{e}") }],
                "isError": true,
            }),
        ),
    }
}

/// Render a tool's JSON output as text for the MCP `content` block.
fn tool_output_text(output: &Value) -> String {
    match output {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    }
}

fn result_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Convenience: serve the default Aonyx tool set (fs / bash / git).
pub async fn serve_default() -> aonyx_core::Result<()> {
    serve_stdio(ToolRegistry::default_set()).await
}

/// Serve the Aonyx tool catalogue over a minimal **Streamable HTTP**
/// transport (Phase OO) — the server-side mirror of the HTTP client
/// shipped in Phase II.
///
/// Intentionally tiny: HTTP/1.1, one JSON-RPC message POSTed per
/// request, `Content-Length` framing, `Connection: close` (no
/// keep-alive, no chunked encoding). Each connection is handled on its
/// own task. A `GET` (or any non-POST) is treated as a health probe.
/// Loops until the listener errors.
pub async fn serve_http(
    registry: ToolRegistry,
    addr: &str,
    token: Option<String>,
) -> aonyx_core::Result<()> {
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| aonyx_core::AonyxError::Mcp(format!("bind {addr}: {e}")))?;
    loop {
        let (stream, _peer) = listener
            .accept()
            .await
            .map_err(|e| aonyx_core::AonyxError::Mcp(format!("accept: {e}")))?;
        let reg = registry.clone();
        let tok = token.clone();
        tokio::spawn(async move {
            let _ = serve_http_conn(stream, &reg, tok.as_deref()).await;
        });
    }
}

/// Read one HTTP request off `stream`, dispatch it, write the reply.
async fn serve_http_conn(
    mut stream: TcpStream,
    registry: &ToolRegistry,
    token: Option<&str>,
) -> aonyx_core::Result<()> {
    let raw = read_http_request(&mut stream).await?;
    let resp = http_response_for(&raw, registry, token).await;
    stream
        .write_all(resp.as_bytes())
        .await
        .map_err(|e| aonyx_core::AonyxError::Mcp(format!("write http: {e}")))?;
    stream
        .flush()
        .await
        .map_err(|e| aonyx_core::AonyxError::Mcp(format!("flush http: {e}")))?;
    Ok(())
}

/// Read a full HTTP request (headers + `Content-Length` body) into a
/// byte buffer. Bounded so a hostile client can't exhaust memory.
async fn read_http_request(stream: &mut TcpStream) -> aonyx_core::Result<Vec<u8>> {
    const MAX_HEADER: usize = 16 * 1024;
    const MAX_BODY: usize = 16 * 1024 * 1024;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if let Some(p) = find_subsequence(&buf, b"\r\n\r\n") {
            break p + 4;
        }
        if buf.len() > MAX_HEADER {
            return Ok(buf); // headers too large — bail, handler treats as health.
        }
        let n = stream
            .read(&mut tmp)
            .await
            .map_err(|e| aonyx_core::AonyxError::Mcp(format!("read http: {e}")))?;
        if n == 0 {
            return Ok(buf); // EOF before full headers.
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let want = head_end.saturating_add(parse_content_length(&buf[..head_end]).min(MAX_BODY));
    while buf.len() < want {
        let n = stream
            .read(&mut tmp)
            .await
            .map_err(|e| aonyx_core::AonyxError::Mcp(format!("read http body: {e}")))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    Ok(buf)
}

/// Turn a raw HTTP request into a full HTTP/1.1 response string. Pure
/// (no socket) so it can be unit-tested directly.
///
/// When `token` is `Some`, the request must carry a matching
/// `Authorization: Bearer <token>` header or it is rejected with `401`
/// before any dispatch (Phase PP).
async fn http_response_for(raw: &[u8], registry: &ToolRegistry, token: Option<&str>) -> String {
    if let Some(expected) = token {
        let authorized = header_value(raw, "authorization")
            .map(|v| v == format!("Bearer {expected}"))
            .unwrap_or(false);
        if !authorized {
            let payload = json!({ "error": "unauthorized" }).to_string();
            return format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nWWW-Authenticate: Bearer\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
        }
    }
    let head_end = find_subsequence(raw, b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(raw.len());
    let is_post = first_line(raw).starts_with("POST");
    let body = raw.get(head_end..).unwrap_or(&[]);

    let response_json = if is_post && !body.is_empty() {
        match serde_json::from_slice::<Value>(body) {
            Ok(req) => handle_message(&req, registry).await,
            Err(e) => Some(error_response(
                Value::Null,
                -32700,
                &format!("parse error: {e}"),
            )),
        }
    } else {
        None
    };
    let payload = match response_json {
        Some(v) => v.to_string(),
        // Notification (no id) or non-POST probe — acknowledge with a
        // small health body so clients see a well-formed 200.
        None => json!({ "server": "aonyx-agent", "status": "ok" }).to_string(),
    };
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    )
}

/// Find the first occurrence of `needle` in `haystack`.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Parse the `Content-Length` header value (case-insensitive) from a
/// header block. Returns 0 when absent or unparseable.
fn parse_content_length(headers: &[u8]) -> usize {
    let text = String::from_utf8_lossy(headers);
    for line in text.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                return value.trim().parse().unwrap_or(0);
            }
        }
    }
    0
}

/// Extract the HTTP request line (everything up to the first CRLF).
fn first_line(raw: &[u8]) -> String {
    let end = find_subsequence(raw, b"\r\n").unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

/// Case-insensitive lookup of a header value from the request's header
/// block (Phase PP). Returns the trimmed value, or `None` if absent.
fn header_value(raw: &[u8], name: &str) -> Option<String> {
    let head_end = find_subsequence(raw, b"\r\n\r\n").unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..head_end]);
    head.lines().skip(1).find_map(|line| {
        line.split_once(':').and_then(|(k, v)| {
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_string())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn initialize_returns_server_info() {
        let reg = ToolRegistry::default_set();
        let req = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
        let resp = handle_message(&req, &reg).await.expect("response");
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["serverInfo"]["name"], "aonyx-agent");
        assert_eq!(resp["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn notifications_get_no_reply() {
        let reg = ToolRegistry::default_set();
        let req = json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}});
        assert!(handle_message(&req, &reg).await.is_none());
    }

    #[tokio::test]
    async fn tools_list_enumerates_the_registry() {
        let reg = ToolRegistry::default_set();
        let req = json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}});
        let resp = handle_message(&req, &reg).await.expect("response");
        let tools = resp["result"]["tools"].as_array().expect("array");
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"fs_read"));
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"git_status"));
        // Sorted alphabetically.
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[tokio::test]
    async fn unknown_method_is_method_not_found() {
        let reg = ToolRegistry::default_set();
        let req = json!({"jsonrpc":"2.0","id":3,"method":"frobnicate","params":{}});
        let resp = handle_message(&req, &reg).await.expect("response");
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn tools_call_unknown_tool_errors() {
        let reg = ToolRegistry::default_set();
        let req = json!({
            "jsonrpc":"2.0","id":4,"method":"tools/call",
            "params": { "name": "does_not_exist", "arguments": {} }
        });
        let resp = handle_message(&req, &reg).await.expect("response");
        assert_eq!(resp["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn tools_call_invokes_a_real_tool() {
        // fs_read on this source file should succeed and come back as text.
        let reg = ToolRegistry::default_set();
        let req = json!({
            "jsonrpc":"2.0","id":5,"method":"tools/call",
            "params": { "name": "fs_read", "arguments": { "path": "Cargo.toml" } }
        });
        let resp = handle_message(&req, &reg).await.expect("response");
        // Either it read the file (isError false) or failed gracefully —
        // both must be well-formed MCP content, never a transport error.
        assert!(resp["result"]["content"].is_array());
        assert_eq!(resp["id"], 5);
    }

    #[test]
    fn tool_output_text_passes_strings_through() {
        assert_eq!(tool_output_text(&json!("hello")), "hello");
        assert!(tool_output_text(&json!({"a":1})).contains("\"a\""));
    }

    #[test]
    fn find_subsequence_and_helpers() {
        assert_eq!(find_subsequence(b"abc\r\n\r\nbody", b"\r\n\r\n"), Some(3));
        assert_eq!(find_subsequence(b"no marker", b"\r\n\r\n"), None);
        assert_eq!(
            parse_content_length(b"Host: x\r\nContent-Length: 42\r\n"),
            42
        );
        // Case-insensitive header name.
        assert_eq!(parse_content_length(b"content-length: 7\r\n"), 7);
        assert_eq!(parse_content_length(b"no length here\r\n"), 0);
        assert_eq!(
            first_line(b"POST / HTTP/1.1\r\nHost: x\r\n"),
            "POST / HTTP/1.1"
        );
    }

    #[tokio::test]
    async fn http_response_for_dispatches_jsonrpc_post() {
        let body = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}).to_string();
        let raw = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let resp = http_response_for(raw.as_bytes(), &ToolRegistry::default_set(), None).await;
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("Content-Type: application/json"));
        // The JSON-RPC body must carry the initialize result.
        assert!(resp.contains("aonyx-agent"));
        assert!(resp.contains("protocolVersion"));
    }

    #[tokio::test]
    async fn http_response_for_treats_get_as_health_probe() {
        let raw = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let resp = http_response_for(raw, &ToolRegistry::default_set(), None).await;
        assert!(resp.starts_with("HTTP/1.1 200 OK"));
        assert!(resp.contains("\"status\":\"ok\""));
    }

    #[tokio::test]
    async fn http_server_round_trips_over_a_real_socket() {
        // Exercise the full connection path (read_http_request +
        // http_response_for + write) over loopback TCP, minus the
        // infinite accept loop. `Connection: close` drops the stream so
        // the client's read_to_end terminates — no hang.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_http_conn(stream, &ToolRegistry::default_set(), None)
                .await
                .unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        let body = json!({"jsonrpc":"2.0","id":9,"method":"ping","params":{}}).to_string();
        let req = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        client.write_all(req.as_bytes()).await.unwrap();
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(text.contains("\"id\":9")); // ping reply echoes the id.
        server.await.unwrap();
    }

    #[tokio::test]
    async fn http_response_for_reports_parse_errors() {
        let body = "{not json";
        let raw = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let resp = http_response_for(raw.as_bytes(), &ToolRegistry::default_set(), None).await;
        assert!(resp.contains("-32700")); // JSON-RPC parse error code.
    }

    #[test]
    fn header_value_is_case_insensitive() {
        let raw = b"POST / HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer abc\r\n\r\n";
        assert_eq!(
            header_value(raw, "authorization").as_deref(),
            Some("Bearer abc")
        );
        assert_eq!(
            header_value(raw, "AUTHORIZATION").as_deref(),
            Some("Bearer abc")
        );
        assert_eq!(header_value(raw, "x-missing"), None);
    }

    #[tokio::test]
    async fn http_auth_required_when_token_set() {
        let body = json!({"jsonrpc":"2.0","id":1,"method":"ping"}).to_string();
        let mk = |auth: Option<&str>| {
            let hdr = auth
                .map(|a| format!("Authorization: {a}\r\n"))
                .unwrap_or_default();
            format!(
                "POST / HTTP/1.1\r\nHost: x\r\n{hdr}Content-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
        };
        let reg = ToolRegistry::default_set();
        // Correct token → 200.
        let ok =
            http_response_for(mk(Some("Bearer s3cret")).as_bytes(), &reg, Some("s3cret")).await;
        assert!(ok.starts_with("HTTP/1.1 200"));
        // Wrong token → 401.
        let bad = http_response_for(mk(Some("Bearer nope")).as_bytes(), &reg, Some("s3cret")).await;
        assert!(bad.starts_with("HTTP/1.1 401"));
        // Missing header → 401.
        let none = http_response_for(mk(None).as_bytes(), &reg, Some("s3cret")).await;
        assert!(none.starts_with("HTTP/1.1 401"));
        // No token configured → open (200) even without a header.
        let open = http_response_for(mk(None).as_bytes(), &reg, None).await;
        assert!(open.starts_with("HTTP/1.1 200"));
    }

    #[tokio::test]
    async fn tools_list_includes_registered_memory_tools() {
        // Phase NN — `aonyx mcp serve` folds palace-backed `memory_*`
        // tools into the served registry. Verify they surface through
        // `tools/list` once registered (the wiring `build_serve_registry`
        // performs against the cwd palace).
        use aonyx_memory::Palace;
        let palace = Palace::open_in_memory().expect("palace");
        let mut reg = ToolRegistry::default_set();
        reg.register(std::sync::Arc::new(aonyx_tools::memory::MemorySearch::new(
            palace.clone(),
        )));
        reg.register(std::sync::Arc::new(
            aonyx_tools::memory::MemoryKgQuery::new(palace.kg.clone()),
        ));

        let req = json!({"jsonrpc":"2.0","id":7,"method":"tools/list","params":{}});
        let resp = handle_message(&req, &reg).await.expect("response");
        let tools = resp["result"]["tools"].as_array().expect("array");
        let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"memory_search"));
        assert!(names.contains(&"memory_kg_query"));
        // The static set is still present alongside them.
        assert!(names.contains(&"fs_read"));
    }
}

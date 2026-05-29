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
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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
}

//! Lua plugin tools (Phase WW) — drop a `.lua` file in `~/.aonyx/plugins/`
//! to add a tool in-process, without writing a full MCP server. This
//! complements the MCP client (which already covers subprocess plugins)
//! with lightweight scripting.
//!
//! Each script registers one or more tools via the injected global:
//!
//! ```lua
//! aonyx.register_tool {
//!   name = "weather",
//!   description = "Get the weather for a city",
//!   run = function(args) return "sunny in " .. (args.city or "?") end,
//! }
//! ```
//!
//! `args` is the JSON tool arguments as a Lua table; `run` returns a
//! string (or a table, serialised to JSON). The Lua VM is `!Send`, so it
//! lives on its own dedicated thread reached over a channel — the
//! resulting [`ToolHandler`]s are `Send + Sync` like every other tool.

use std::path::Path;
use std::sync::Arc;

use aonyx_core::{AonyxError, Result, SafetyClass, ToolCall, ToolHandler, ToolResult};
use async_trait::async_trait;
use mlua::LuaSerdeExt;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

/// Metadata for one registered plugin tool.
#[derive(Clone)]
struct PluginSpec {
    name: String,
    description: String,
}

/// An invocation request handed to the Lua host thread.
struct Invoke {
    tool: String,
    args: Value,
    reply: oneshot::Sender<std::result::Result<Value, String>>,
}

/// Load every `*.lua` file under `dir`, run them on a dedicated Lua
/// thread, and return one [`ToolHandler`] per registered tool. Returns an
/// empty vec when the directory is absent or holds no valid plugins.
pub fn load_plugins(dir: &Path) -> Vec<Arc<dyn ToolHandler>> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut scripts: Vec<(String, String)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("lua") {
                if let Ok(src) = std::fs::read_to_string(&path) {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("plugin")
                        .to_string();
                    scripts.push((name, src));
                }
            }
        }
    }
    if scripts.is_empty() {
        return Vec::new();
    }

    let (req_tx, req_rx) = mpsc::unbounded_channel::<Invoke>();
    let (spec_tx, spec_rx) = std::sync::mpsc::channel::<Vec<PluginSpec>>();

    // The Lua state lives entirely on this thread.
    if std::thread::Builder::new()
        .name("aonyx-lua".into())
        .spawn(move || lua_host(scripts, spec_tx, req_rx))
        .is_err()
    {
        return Vec::new();
    }

    let specs = spec_rx.recv().unwrap_or_default();
    specs
        .into_iter()
        .map(|s| {
            Arc::new(LuaTool {
                name: s.name,
                description: s.description,
                tx: req_tx.clone(),
            }) as Arc<dyn ToolHandler>
        })
        .collect()
}

/// Body of the dedicated Lua thread: load the scripts, report the
/// registered specs, then serve invocations until the channel closes.
fn lua_host(
    scripts: Vec<(String, String)>,
    spec_tx: std::sync::mpsc::Sender<Vec<PluginSpec>>,
    mut req_rx: mpsc::UnboundedReceiver<Invoke>,
) {
    let lua = mlua::Lua::new();
    let specs = match register_scripts(&lua, &scripts) {
        Ok(specs) => specs,
        Err(e) => {
            tracing::warn!("lua plugins: failed to initialise: {e}");
            let _ = spec_tx.send(Vec::new());
            return;
        }
    };
    if specs.is_empty() {
        let _ = spec_tx.send(Vec::new());
        return;
    }
    let _ = spec_tx.send(specs);

    while let Some(req) = req_rx.blocking_recv() {
        let result = call_tool(&lua, &req.tool, req.args);
        let _ = req.reply.send(result);
    }
}

/// Inject the `aonyx.register_tool` global, run each script, and collect
/// the registered tool specs. Each tool's `run` function is stashed in the
/// `__aonyx_tools` Lua table for later dispatch by name.
fn register_scripts(
    lua: &mlua::Lua,
    scripts: &[(String, String)],
) -> mlua::Result<Vec<PluginSpec>> {
    use std::cell::RefCell;
    use std::rc::Rc;

    let collected: Rc<RefCell<Vec<PluginSpec>>> = Rc::new(RefCell::new(Vec::new()));
    lua.globals().set("__aonyx_tools", lua.create_table()?)?;

    let coll = Rc::clone(&collected);
    let register = lua.create_function(move |lua, spec: mlua::Table| {
        let name: String = spec.get("name")?;
        let description: String = spec.get("description").unwrap_or_default();
        let run: mlua::Function = spec.get("run")?;
        let tools: mlua::Table = lua.globals().get("__aonyx_tools")?;
        tools.set(name.clone(), run)?;
        coll.borrow_mut().push(PluginSpec { name, description });
        Ok(())
    })?;

    let aonyx = lua.create_table()?;
    aonyx.set("register_tool", register)?;
    lua.globals().set("aonyx", aonyx)?;

    for (file, src) in scripts {
        if let Err(e) = lua.load(src.as_str()).set_name(file.as_str()).exec() {
            tracing::warn!("lua plugin '{file}': {e}");
        }
    }

    let specs = collected.borrow().clone();
    Ok(specs)
}

/// Call a registered Lua tool with JSON `args`, returning its JSON result.
fn call_tool(lua: &mlua::Lua, name: &str, args: Value) -> std::result::Result<Value, String> {
    let tools: mlua::Table = lua
        .globals()
        .get("__aonyx_tools")
        .map_err(|e| e.to_string())?;
    let run: mlua::Function = tools
        .get(name)
        .map_err(|_| format!("plugin tool '{name}' not found"))?;
    let lua_args = lua.to_value(&args).map_err(|e| e.to_string())?;
    let ret: mlua::Value = run.call(lua_args).map_err(|e| e.to_string())?;
    lua.from_value(ret).map_err(|e| e.to_string())
}

/// A [`ToolHandler`] backed by a Lua function running on the host thread.
struct LuaTool {
    name: String,
    description: String,
    tx: mpsc::UnboundedSender<Invoke>,
}

#[async_trait]
impl ToolHandler for LuaTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn classify(&self) -> SafetyClass {
        // User-authored scripts living in the user's own config dir:
        // surfaced as cautionary, not gated like a destructive built-in.
        SafetyClass::Caution
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "description": self.description,
            "properties": {},
            "additionalProperties": true
        })
    }

    async fn invoke(&self, call: ToolCall) -> Result<ToolResult> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Invoke {
                tool: self.name.clone(),
                args: call.args,
                reply,
            })
            .map_err(|_| {
                AonyxError::Tool(format!("lua plugin '{}': host thread gone", self.name))
            })?;
        let out = rx
            .await
            .map_err(|_| AonyxError::Tool(format!("lua plugin '{}': no reply", self.name)))?;
        match out {
            Ok(value) => Ok(ToolResult {
                call_id: call.id,
                output: value,
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                call_id: call.id,
                output: Value::Null,
                error: Some(e),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aonyx_core::ToolCall;

    #[tokio::test]
    async fn loads_and_invokes_a_lua_tool() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("echo.lua"),
            r#"
            aonyx.register_tool {
              name = "echo",
              description = "echo back the message",
              run = function(args) return "you said: " .. (args.message or "") end,
            }
            "#,
        )
        .unwrap();

        let tools = load_plugins(dir.path());
        assert_eq!(tools.len(), 1);
        let echo = tools.into_iter().find(|t| t.name() == "echo").unwrap();
        assert!(matches!(echo.classify(), SafetyClass::Caution));

        let res = echo
            .invoke(ToolCall {
                id: "1".into(),
                name: "echo".into(),
                args: json!({ "message": "hi" }),
            })
            .await
            .unwrap();
        assert_eq!(res.output, json!("you said: hi"));
        assert!(res.error.is_none());
    }

    #[test]
    fn missing_dir_is_empty() {
        let tools = load_plugins(Path::new("/no/such/aonyx/plugins/dir"));
        assert!(tools.is_empty());
    }
}

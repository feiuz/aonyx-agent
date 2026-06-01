-- Example Aonyx Lua plugin (Phase WW).
--
-- Copy this file into ~/.aonyx/plugins/ and run an `aonyx` built with the
-- `lua-plugins` feature (`cargo install aonyx-agent --features lua-plugins`).
-- The agent then gains a `hello` tool it can call mid-conversation.
--
-- Contract: call aonyx.register_tool with a table holding
--   name        : string  — the tool name the model invokes
--   description : string  — shown to the model
--   run         : function(args) -> string|table
--                 `args` is the JSON tool arguments as a Lua table;
--                 return a string (or a table, serialised to JSON).

aonyx.register_tool {
  name = "hello",
  description = "Greet someone by name.",
  run = function(args)
    return "Hello, " .. (args.name or "world") .. "!"
  end,
}

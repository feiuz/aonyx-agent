# BMAD — Plan: real end-to-end tool calling

**Project**: Aonyx Agent
**Date**: 2026-06-03
**Status**: Planned → in progress. Targets **v0.8.0**.
**Severity**: 🔴 Blocker — without this the agent cannot call any tool
(built-in, MCP, or plugin). It hallucinates instead of querying. Reported
from a real deployment (local llama-server + MCP `aonyx-rag`): native tool
calls verified at the model, but `aonyx` never sends/parses them.

---

## Root cause (verified in source)

The **runner is fully wired** — `runner.rs` builds `tools_schema()`, puts it
in `ChatRequest.tools`, collects `chunk.tool_call`, and loops invoking tools.
The gap is entirely downstream:

1. **`openai_compat.rs`** (openai / openrouter / ollama / lm-studio /
   nous-portal) — payload omits `tools`; SSE parser hard-codes
   `tool_call: None`. Comment: *"deferred to P3"*. So no tool is ever
   offered to or returned by the model.
2. **`anthropic.rs`** — sends `tools` but the SSE parser hard-codes
   `tool_call: None` (never parses `tool_use`). Comment: *"V1.1 will emit
   proper tool_use"*.
3. **`aonyx-core::Message`** has **no tool-call structure** (`id, role,
   content, ts, attachments`). The runner therefore records a tool request
   as plain assistant *text* and the result as a `Role::Tool` *text* message
   with no `tool_call_id`. Even once parsing works, the multi-turn replay is
   malformed for both providers.

Bonus defect: `tools_schema()` sends `"description": ""` — the model gets no
hint what each tool does.

## Design

Add structured tool-call data to the core message, record it in the loop,
and teach both provider wire-formats to serialize **and** parse it.

### T1 — `aonyx-core`: structured tool calls on `Message`
- Add `#[serde(default)] tool_calls: Vec<ToolCall>` (set on assistant
  messages that requested tools) and `#[serde(default)] tool_call_id:
  Option<String>` (set on `Role::Tool` results).
- Add constructors: `Message::assistant_tool_calls(text, calls)` and
  `Message::tool_result(call_id, content)`. Keep `new` / `with_attachments`
  unchanged. Defaults keep persisted sessions (sessions.db, bundles)
  deserialising unchanged.

### T2 — `runner`: record + link
- After `consume_stream`, push **one** assistant message carrying the text
  *and* the `tool_calls` (not text-only).
- Push each tool result as `Message::tool_result(call.id, payload)` so the
  `tool_call_id` links back.
- `tools_schema()`: fill `description` from `handler.schema()["description"]`
  when present.

### T3 — `openai_compat`: OpenAI function-calling
- **Request**: translate the runner's Anthropic-shaped tools
  (`{name, description, input_schema}`) → OpenAI
  `{type:"function", function:{name, description, parameters}}`. Serialize
  assistant `tool_calls` → `{role:"assistant", content, tool_calls:[{id,
  type:"function", function:{name, arguments:<json string>}}]}`; tool
  results → `{role:"tool", tool_call_id, content}`.
- **Response**: accumulate streamed `delta.tool_calls[]` by `index`
  (id + name + argument fragments), emit a `ToolCall` per index when the
  stream finishes (`finish_reason == "tool_calls"`). This is exactly the
  shape llama-server emits.

### T4 — `anthropic`: tool_use / tool_result blocks
- **Request**: assistant `tool_calls` → content blocks `[{type:"text"},
  {type:"tool_use", id, name, input}]`; tool results (`Role::Tool`) → a
  `user` message with `[{type:"tool_result", tool_use_id, content}]`.
- **Response**: parse the streamed tool_use — `content_block_start`
  (`{type:"tool_use", id, name}`) → accumulate `input_json_delta`
  (`partial_json`) → emit a `ToolCall` at `content_block_stop`.

### T5 — tests, verify, release
- Unit tests: openai_compat (parse a multi-chunk `tool_calls` stream →
  `ToolCall`; request serialization incl. tools + tool_calls + tool role);
  anthropic (parse `tool_use` stream; request block serialization); runner
  (mock provider emitting a tool call → assistant msg has `tool_calls`,
  result msg has `tool_call_id`).
- `cargo test --workspace` + `clippy --all-features -D warnings`.
- Manual: `aonyx serve api` / TUI against a tool-using prompt.
- CHANGELOG + docs; cut **v0.8.0**; the desktop work shifts to v0.9.0.

## Non-goals
- Parallel/streamed partial tool execution (we collect all calls per
  assistant turn, then run them — matches the current loop).
- Provider-specific niceties (Anthropic fine-grained streaming of tool
  text vs input) beyond what the loop needs.

## Risk
- **Backward compat of sessions**: mitigated by `#[serde(default)]` on the
  new `Message` fields — old rows decode with empty tool data.
- **Wire-format drift**: covered by parse/serialize unit tests with real
  sample payloads from both APIs (incl. llama.cpp's OpenAI shape).

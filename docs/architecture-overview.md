# Architecture overview

This is the user-facing entry point. The deep design rationale lives in
[`.bmad/architecture.md`](../.bmad/architecture.md).

## In one diagram

```
   you  ──▶  aonyx (CLI)
              │
              ▼
        ┌────────────┐
        │  agent loop │
        └─┬───┬───┬───┘
          │   │   │
   ┌──────┘   │   └──────┐
   ▼          ▼          ▼
  LLM       tools     memory ⭐
  router  fs/bash/…  KG + diary
                     + hybrid search
                     + time-machine
```

## Reading order

1. [Brief](../.bmad/brief.md) — why this project exists.
2. [PRD](../.bmad/prd.md) — what V1 is, what it is not.
3. [Architecture](../.bmad/architecture.md) — how every crate fits.
4. [Decisions](../.bmad/decisions.md) — the trade-offs we have already taken.

## Contributing

The project is in **pre-alpha scaffolding**. Issues and PRs are welcome once the
public repository is created. For now, the workspace structure is:

```
crates/
├── aonyx-core/        # shared types, traits, errors
├── aonyx-memory/      # palace: KG + diary + hybrid + time-machine
├── aonyx-llm/         # multi-provider router
├── aonyx-tools/       # built-in tools
├── aonyx-skills/      # SKILL.md engine + 4 built-ins
├── aonyx-agent/       # loop + compaction + classifier + approval
├── aonyx-mcp/         # client + server
├── aonyx-cli/         # the `aonyx` binary
├── aonyx-tui/         # ratatui UI (V1.5)
└── aonyx-adapters/    # Telegram / Discord / OpenAI HTTP (V2)
```

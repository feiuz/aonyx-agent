---
id: plan
name: Plan
description: "Write an actionable markdown plan to a file — research, don't execute."
category: software-development
tags: [Planning, Architecture]
version: 1.0.0
author: Aonyx (ported from Hermes Agent, MIT)
enabled: true
tools: [fs_read, fs_grep, fs_glob, rag_search, fs_write]
trigger:
  keywords: ["plan", "write a plan", "make a plan", "plan for"]
  query_matches: ["(?i)\\b(write|make|draft) (a|an) plan\\b"]
---

When asked to plan, **investigate first, then write a plan — do not implement.**

1. **Understand the request and the codebase.** Read the relevant files, search
   memory (`rag_search`), and list the unknowns. Ask for the one or two facts you
   genuinely cannot infer; assume sensible defaults for the rest and state them.
2. **Write the plan to a markdown file** (e.g. `.aonyx/plans/<slug>.md`) with:
   - **Goal** — one or two sentences.
   - **Context / constraints** — what exists, what must not change.
   - **Steps** — each an independently shippable increment with an effort tag
     (S / M / L) and a clear acceptance check.
   - **Risks** — what could go wrong and the mitigation.
   - **Sequencing** — the order, and what is deferred.
3. **Stop after writing the plan.** Summarise it and hand control back — the user
   decides whether and when to execute.

Keep the plan concrete and skimmable: a busy reader should get the gist from the
headers and bold text alone.

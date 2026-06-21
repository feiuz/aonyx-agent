---
id: systematic-debugging
name: Systematic Debugging
description: "Root-cause a bug in four phases — understand it before you fix it."
category: software-development
tags: [Debugging, Root-Cause]
version: 1.0.0
author: Aonyx (ported from Hermes Agent, MIT)
enabled: true
tools: [fs_read, fs_grep, fs_glob, git_log, git_diff, bash]
trigger:
  keywords: ["debug", "bug", "why is this failing", "root cause", "broken"]
  query_matches: ["(?i)\\b(debug|root cause|why (is|does).*(fail|break|crash))\\b"]
---

Don't patch symptoms. Work the four phases in order — most bugs die in phase 1.

1. **Reproduce & observe.** Get a deterministic repro. Read the actual error, the
   stack, and the inputs. Note what you *see*, not what you assume.
2. **Locate.** Bisect the failure: narrow the code path with `git_log`/`git_diff`
   (what changed?), targeted reads, and minimal probes. Form one hypothesis at a
   time and test it — confirm the faulty line before theorising about a fix.
3. **Explain the root cause.** State *why* it fails in one sentence. If you can't,
   you haven't found it yet — go back to phase 2. Check for the same bug elsewhere.
4. **Fix & verify.** Make the smallest change that addresses the cause (not the
   symptom). Re-run the repro to confirm it's gone, and run the surrounding tests
   so the fix doesn't regress anything.

Prefer evidence (logs, diffs, a failing test) over speculation at every step.

---
id: hir-v3pv
status: closed
deps: []
links: [hir-jgs1]
created: 2026-05-22T21:43:16Z
type: task
priority: 1
assignee: nomaterials
tags: [decision, design, tools, replay]
---
# OD4: Tool effect replay semantics

Resolve whether replay re-executes tools or returns logged values.

**Recommended resolution**: both, as handler choices.

- **Replay mode** (for audit and deterministic testing): a replay handler reads
  the audit log and returns logged values for tool effects. The execution is
  deterministic and matches the original run exactly.
- **Re-execute mode** (for live debugging): tools are called again. Results may
  differ from the original run. Useful for testing with live services.
- The choice is a handler decision: install a replay handler or a live handler.
  The language doesn't have a special "replay" mode; it's just a handler swap.

This means the audit log must contain enough information to replay: full
structured arguments and full structured results for every tool invocation.

**Decision point**: Phase 6 implementation.

## Acceptance Criteria

- Decision documented in DECISIONS.md.
- Replay handler implemented and tested.
- Audit log contains sufficient information for replay.


## Notes

**2026-07-02T13:18:20Z**

Resolution locked in hir-jgs1 (council, 2026-07-02): both modes as handler
choices, as recommended. Replay handler semantics pinned to strict-sequential
matching with a hard structured Divergence error on mismatch;
keyed-by-(tool,args) matching and live fall-through are explicitly out of
scope (nondeterminism). Log contains full args and tagged ok/err results,
sufficient for replay including failures. Only divergence-reporting
ergonomics remain provisional pending real runs. ADR to be written in
DECISIONS.md during hir-jgs1 implementation.

**2026-07-03T09:44:45Z**

Resolved and documented as ADR-016 in DECISIONS.md, implemented in hir-jgs1
(commits bea7971, f388ddf): replay returns logged values, re-execute is the
same program under a live handler (handler choice, not a language mode); core
is the pure function (log, position, tool, args) -> Result<result, Divergence>
with strict-sequential matching and hard structured Divergence on
exhausted/tool/args mismatch. Log carries full args and tagged ok/err results,
so failures replay faithfully. Divergence-reporting ergonomics remain
provisional pending real runs.

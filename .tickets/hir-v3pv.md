---
id: hir-v3pv
status: open
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


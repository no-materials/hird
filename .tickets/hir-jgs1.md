---
id: hir-jgs1
status: open
deps: [hir-4g3y]
links: [hir-yum3, hir-v3pv]
created: 2026-05-22T21:39:42Z
type: task
priority: 1
assignee: nomaterials
parent: hir-jt39
tags: [phase-6, tools, audit]
---
# Audit log integration and tool effect docs

Implement the audit log as a first-class language concept and write the
comprehensive tool-effects documentation.

**Audit log semantics**:
- Every tool effect invocation produces a structured invocation record.
- Records are emitted to an audit sink (configurable via handler).
- Default audit sink: JSON lines to stdout or a file.
- Record format:
  ```json
  {
    "tool": "ReadRepo",
    "args": { "path": "/home/user/repo" },
    "result": { "files": [...], "status": "clean" },
    "timestamp": "2026-05-22T12:00:00Z",
    "caller": "Planner.handle_msg/PlanRepo",
    "duration_ms": 42
  }
  ```
- The audit sink is itself a capability: `AuditSink` is passed in, not ambient.

**Replay semantics** (resolves OD4):
- Audit logs can be replayed: feed a log back to the runtime, and tool effects
  return logged values instead of re-executing.
- This is "replay from log" mode — deterministic, suitable for testing and audit.
- Alternative "re-execute" mode is the default for live runs.
- The choice between replay and re-execute is a handler decision, not a language
  feature — a replay handler reads the log; a live handler calls the real tool.

**Documentation** (docs/tool-effects.md):
- What tool effects are and why they exist.
- How tool declarations work (syntax, generated types, invocation records).
- How handlers interact with tool effects (DI-style replacement, mocking).
- Audit log format specification.
- Replay semantics (OD4 resolution).
- Comparison with regular effects.
- LLM-specific guidance: how to declare tools for LLM-mediated operations.
- Examples: the planner demo's tool declarations annotated.

This ticket resolves **OD3 (Audit log fidelity)** and **OD4 (Replay semantics)**.

## Acceptance Criteria

- Tool invocations produce structured JSON invocation records.
- Audit sink is capability-based (AuditSink parameter, not ambient).
- Default audit sink writes JSON lines.
- Replay handler: given a log, returns logged values for tool effects.
- docs/tool-effects.md written with all sections listed above.
- OD3 and OD4 documented in DECISIONS.md.
- Snapshot tests: invocation record format, audit log output for a sequence
  of tool calls, replay handler returning logged values.
- At least 6 snapshot tests.


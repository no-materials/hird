---
id: hir-yum3
status: open
deps: []
links: [hir-jgs1]
created: 2026-05-22T21:43:09Z
type: task
priority: 1
assignee: nomaterials
tags: [decision, design, audit]
---
# OD3: Audit log fidelity

Resolve what guarantees the audit log provides.

**Recommended resolution**: structured JSON logging with clear upgrade path.

v0.1 audit log:
- JSON lines format, one record per tool invocation.
- Fields: tool name, structured args, structured result, timestamp, caller ID,
  duration.
- Written to configurable sink (file or stdout).
- No tamper-proofing, no content addressing, no Merkle chaining.
- Deterministic field ordering for diffability.

Upgrade path (v0.2+):
- Content-addressed records (hash each record).
- Chained records (each record includes the hash of the previous).
- Signature support for audit records.
- Binary format option for performance.

The v0.1 level is sufficient for testing, debugging, and basic audit. The
upgrade path is documented but not implemented.

**Decision point**: Phase 6 implementation.

## Acceptance Criteria

- Decision documented in DECISIONS.md.
- Audit log format specified in docs/tool-effects.md.
- Implementation matches the specified format.


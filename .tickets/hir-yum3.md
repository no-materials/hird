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


## Notes

**2026-07-02T13:18:20Z**

Resolution locked in hir-jgs1 (council, 2026-07-02), matching this ticket's
recommendation: JSON-lines, deterministic byte-exact ordering (hand-rolled
canonical writer, sorted labels for value records / fixed envelope order),
required schema_version field for the upgrade path, no tamper-proofing in
v0.1. Additions beyond the recommendation: tagged ok/err result encoding and
a checker-enforced wire-representability constraint on tool signatures. ADR
to be written in DECISIONS.md during hir-jgs1 implementation.

---
id: hir-actn
status: open
deps: []
links: [hir-m6ra]
created: 2026-05-22T21:43:50Z
type: task
priority: 1
assignee: nomaterials
tags: [decision, design, actors, effects]
---
# OD8: Send/reply effect tracking

Resolve how send and reply effects are represented in effect rows.

**Proposed resolution**:

- send(pid, msg) has effect {Send<Msg>} where Msg is the message type.
  The effect is NOT parameterized by the recipient Pid (Pids are runtime values).
- request(pid, msg_fn) has effects {Send<Msg>, Await<Reply>}.
  Await<Reply> is a separate effect representing the blocking wait for a reply.
- Send and Await are simple effects, not capability-linked (unlike Tool effects,
  which are linked to specific tool declarations).

**Transitive effect closure** is a tooling concern, not a type-system concern:
- The type system tracks local effects: what THIS process does.
- The MCP server can compute transitive closure: "what does this function plus
  everything it sends to do?" by walking the actor graph.
- This separation keeps the type system tractable.

**Alternative considered**:
- Send<Pid, Msg> parameterized by recipient — rejected because Pids are runtime
  values and the type system can't meaningfully track them.
- Request<Msg, Reply> as a single combined effect — simpler but loses the
  distinction between the send and the blocking wait, which have different
  implications for concurrency analysis.

**Decision point**: Phase 5 (effect design), Phase 7 (actor implementation).

## Acceptance Criteria

- Decision documented in DECISIONS.md.
- send and request have the specified effect signatures.
- Transitive closure is a tooling feature, not a type feature.


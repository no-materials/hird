---
id: hir-b2gn
status: open
deps: []
links: [hir-1fif]
created: 2026-05-22T21:43:26Z
type: task
priority: 2
assignee: nomaterials
tags: [decision, design, actors]
---
# OD5: Actor protocol typing richness

Resolve how rich the actor type system is in v0.1.

**Resolution**: start minimal. Sum-type mailboxes only.

v0.1: an actor has a typed mailbox accepting a sum type of messages. The type
system checks exhaustiveness of message handlers and correctness of send/request
types. This is sufficient for the planner demo and covers the 80% case.

Deferred to future work:
- Session types: typed state machines describing legal message sequences.
  (e.g., an actor that must receive Init before it can receive Work.)
- Protocol typing: describing legal interaction patterns between actors.
- Behavioral types: describing the relationship between request and response
  types in multi-step protocols.

The v0.1 design should not preclude adding session types later. The actor
declaration syntax should have room for protocol annotations. But no protocol
typing is implemented.

**Decision point**: Phase 7 implementation.

## Acceptance Criteria

- Decision documented in DECISIONS.md.
- Actor implementation uses sum-type mailboxes.
- Future-work section in ARCHITECTURE.md mentions session types as an extension point.


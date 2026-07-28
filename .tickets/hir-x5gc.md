---
id: hir-x5gc
status: closed
deps: []
links: []
created: 2026-07-28T07:59:04Z
type: task
priority: 0
assignee: nomaterials
parent: hir-xixm
tags: [supervision, design]
---
# Lock the supervisor runtime surface ADR

Review and lock the drafted ADR (next number in DECISIONS.md) for the
`supervise` and `child` keyword forms. The draft's decision points, each
needing sign-off:

1. `supervise(SupName)` — resolves in the supervisor namespace, starts
   the tree via the emitted module's `start_link/0`, returns `()`. One
   instance per declaration: `{local, Module}` registration means a
   second supervise crashes with `{already_started, …}`, which the ADR
   accepts rather than papers over. Carries a checker-known bare
   `Supervise` effect (the Install precedent): process-tree creation is
   global state, so the row records it; `hird run`'s entry check is
   unchanged (it forbids residual `Tool<…>` only).

2. `child(SupName, child_id)` — the supervisor name resolves in its
   namespace, the child id is checked against that supervisor's declared
   children, and the result type is `Pid<Msg>` derived from the child's
   actor's message type. Lowers through `hird_sup_util:child_pid/2` on
   the registered name; a missing or restarting child crashes
   (`{no_child, Id}`) per ADR-021 — tree health is supervision's concern,
   not a caller-recoverable domain error.

3. `child` carries the empty effect row. The lookup creates nothing and
   sends nothing; its nondeterminism is the same "pids are runtime
   values" concession ADR-019 already makes, and there is nothing for a
   handler to intercept. This is the most debatable clause — the
   alternative is a bare row marker for explicitness; settle it at
   review.

4. No first-class supervisor values, consistent with ADR-018; additive
   to lift later.

Alternatives the draft rejects: overloading `spawn` for supervisors (a
supervisor has no mailbox, so a `Pid<Msg>`-shaped result would lie),
`Option<Pid<…>>` from `child` (forces callers to handle what they cannot
recover from, contra ADR-021), implicit boot-time start (rejected on
hir-y9jo), and retrying/awaiting lookup semantics (cannot distinguish
the pre-crash pid from the restarted one, so it buys no determinism).

## Acceptance Criteria

- The ADR is committed to DECISIONS.md as Accepted, with the
  supersession/refinement relationships to ADR-018/020/021 stated.
- Each of the four decision points above is either confirmed or
  consciously amended at review.


## Notes

**2026-07-28T08:04:40Z**

Locked as ADR-024 with all four decision points confirmed at review
unamended, including clause 4 (child is effect-free); the bare-marker
alternative was considered and declined.

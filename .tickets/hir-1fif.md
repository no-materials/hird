---
id: hir-1fif
status: open
deps: [hir-t1cj, hir-a6lz]
links: [hir-b2gn]
created: 2026-05-22T21:40:00Z
type: task
priority: 1
assignee: nomaterials
parent: hir-y85q
tags: [phase-7, actors]
---
# Actor declarations, typed Pid, and spawn

Implement actor declarations as first-class language primitives with typed
mailboxes, typed Pid references, and the spawn primitive.

**Actor declaration syntax**:
```
actor Planner {
  state: PlannerState,

  message: PlannerMsg =
    | PlanRepo(Path)
    | GetStatus(ReplyTo<PlannerStatus>)
    | Shutdown,

  init: fn(config: PlannerConfig) -> PlannerState ! {Log},

  handle PlanRepo(path) -> PlannerState ! {Tool<ReadRepo>, Tool<CreateTicket>, Log} {
    ...
  },

  handle GetStatus(reply_to) -> PlannerState ! {Send<PlannerStatus>} {
    ...
  },

  handle Shutdown -> PlannerState ! {} {
    ...
  },
} ! {Tool<ReadRepo>, Tool<CreateTicket>, Log, Send<PlannerStatus>}
```

**Typed references**:
- Pid<Msg> — a process identifier typed by the message type it accepts.
  Pid<PlannerMsg> can receive PlannerMsg values only.
- ReplyTo<T> — a typed reply channel. Included as a field in a message
  constructor for request/reply patterns. Internally a Pid<T> or a from-ref.

**Spawn primitive**:
- spawn(ActorModule, init_args) -> Pid<Msg> ! {Spawn<Msg>}
- Type-checked: init_args must match the actor's init function signature.
- Returns a typed Pid.
- Has the Spawn<Msg> effect.

**State encapsulation**:
- Actor state is only accessible within the actor's message handlers.
- The state type is part of the actor declaration but not part of its public type.
- External code sees the actor's message type and effect summary, not its state.

This ticket resolves **OD5** (start minimal — sum-type mailboxes, no session types).

## Acceptance Criteria

- actor declaration syntax parsed and type-checked.
- Message type is a sum type with constructors; type-checked as ADT.
- Pid<Msg> type exists; spawn returns Pid<Msg>.
- ReplyTo<T> type exists and is usable in message constructors.
- spawn type-checks init_args against actor's init signature.
- Actor state is encapsulated: accessing state outside handlers is compile error.
- Per-actor effect summary declared and validated against handler bodies.
- OD5 documented in DECISIONS.md.
- IR includes actor nodes.
- Snapshot tests: actor declaration, typed spawn, state encapsulation violation,
  effect summary mismatch, ReplyTo usage.
- At least 10 snapshot tests.


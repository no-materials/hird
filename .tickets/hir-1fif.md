---
id: hir-1fif
status: closed
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

  init: fn(config: PlannerConfig) -> PlannerState ! {Log} = initial_state(config),

  handle PlanRepo(path), st -> PlannerState
    ! {Tool<ReadRepo>, Tool<CreateTicket>, Log} = plan_repo(path, st),

  handle GetStatus(reply_to), st -> PlannerState
    ! {Send<PlannerStatus>} = reply_status(reply_to, st),

  handle Shutdown, st -> PlannerState ! {} = st,
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

## Decisions (pre-implementation)

1. **Handler bodies are bare expressions.** The original braced examples
   (`handle X(p) -> T ! {…} { ... }`) contradicted ADR-009, which reserves
   braces for non-expression positions and states the handler grammar follows
   the bare-body rule. Handlers are `handle Ctor(payload), st -> T ! {…} = e`;
   the example above and phrasebook.md are corrected accordingly. `init` is an
   anonymous `fn(params) -> T ! {…} = e` in the same style.

2. **Current state is an explicit trailing pattern.** No implicit `state`
   binding in handler bodies (explicit-over-implicit). Each handler binds the
   message payload pattern, then the current state as a final comma-separated
   pattern, typed by the declared `state` type. The comma is unambiguous:
   inside a `handle` member the only continuations after the message pattern
   are `,` or `->`, and the member cannot end before its `= e` body.

3. **`spawn` is a keyword form, not a function.** Its first argument is an
   actor name resolved in the actor namespace; actor names are not first-class
   values (consistent with ADR-010's no-first-class-modules stance). The
   checker types `spawn(Actor, args…)` against the actor's init signature.

4. **Builtin provenance.** `Pid<t>` and `ReplyTo<t>` join the built-in type
   constructors in the checker registry (the `List`/`Option` precedent).
   `ReplyTo<t>` is a distinct type, not an alias of `Pid<t>`; its runtime
   representation (pid vs from-ref) is a codegen decision deferred to
   hir-1dvq. `Spawn<t>`/`Send<t>`/`Await<t>` follow the `Tool<t>` precedent:
   ordinary `effect`-declared heads whose semantics the checker knows.
   Pre-registering them can be revisited if a prelude ever exists.

5. **Scope boundaries with hir-m6ra.** Exhaustiveness (missing handlers) is
   hir-m6ra; this ticket errors only on duplicate handlers and handlers naming
   unknown constructors. Since `send` does not exist until hir-m6ra, the
   effect-summary-mismatch tests here exercise `Tool`/`Log` effects; `Send`
   validation lands with the primitives.

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


## Notes

**2026-07-07T06:58:18Z**

Implemented: actor declarations parse/check/lower end to end; Pid/ReplyTo builtins; spawn keyword form typed Pid<Msg> ! {Spawn<Msg>}; state encapsulation (C0040); effect summary equality (C0038); duplicate/foreign-handler errors (C0036/C0037); 24 checker snapshot tests plus parser, AST, IR lowering and round-trip coverage. OD5 resolved in DECISIONS.md ADR-018 (sum-type mailboxes only).

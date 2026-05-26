---
id: hir-y85q
status: open
deps: [hir-rlo4, hir-0rzf]
links: []
created: 2026-05-22T21:34:05Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-7, actors]
---
# Phase 7 — Actors

## Goal

Implement typed actors as first-class language primitives: actor declarations,
typed mailboxes, typed message protocols, typed Pid references, and spawn/send/
request primitives with full type and effect checking.

## v0.1 demo relevance

The planner demo IS an actor. The Planner actor receives a repository path,
processes it through tool effects, and produces tickets. Its message type is a
sum type; its mailbox is typed; its Pid is typed. The supervisor (Phase 8)
supervises it. Without typed actors, the demo doesn't exist.

## Design context

An actor in Hirð is declared with:
- A state type (internal, inaccessible outside the actor).
- A message type (a sum type of all messages the actor can receive).
- An init function producing the initial state.
- Message handlers, one per message constructor, checked for exhaustiveness.
- An effect summary declaring what the actor does (Tool<X>, Log, etc.).

Typed references:
- `Pid<Msg>` — a reference to an actor accepting messages of type Msg.
- `ReplyTo<T>` — a typed reply channel embedded in a message for request/reply.
- `spawn` returns `Pid<Msg>` with a `{Spawn<Msg>}` effect.
- `send` takes `Pid<Msg>` and a value of type Msg, with a `{Send<Msg>}` effect.
- `request` combines send and await-reply, with `{Send<Msg>, Await<Reply>}` effects.

The compiler enforces:
- Actor state is only accessible from within the actor's message handlers.
- Receive clauses are exhaustive against the message type's constructors.
- Per-actor effect summaries match the actual effects of the handler bodies.

Codegen lowers actors to Erlang gen_server-style behavior modules.

## Task sequence

1. [ ] [hir-1fif](hir-1fif.md) — Actor declarations, typed Pid, and spawn
2. [ ] [hir-m6ra](hir-m6ra.md) — Send, request, and receive exhaustiveness
3. [ ] [hir-1dvq](hir-1dvq.md) — Actor codegen to Erlang gen_server

## Open design question

- **OD5 (Actor protocol typing richness)**: Start minimal — actors have a typed
  mailbox accepting a sum type. Typed session-protocol-like state machines are
  future work. Document this scoping decision.

## Out of scope

- Session types or protocol typing beyond sum-type mailboxes.
- Distributed actor references (node-qualified Pids).
- Actor discovery or registry primitives.
- Hot code loading semantics.

## Acceptance Criteria

- `actor` declaration syntax parsed and type-checked: actor type, message type,
  state type, init function, message handlers.
- `Pid<Msg>` type for typed actor references.
- `ReplyTo<T>` type for typed reply channels.
- `spawn` primitive returning typed Pid<Msg> with {Spawn<Msg>} effect.
- `send` primitive with {Send<Msg>} effect, type-checked against Pid<Msg>.
- `request` primitive with {Send<Msg>, Await<Reply>} effect.
- Compiler enforces actor state encapsulation: accessing state outside handlers
  is a compile error.
- Receive clause exhaustiveness: missing message constructors produce compile
  errors listing the unhandled variants.
- Per-actor effect summaries: declared effects on actor match actual handler
  effects; mismatches produce errors.
- IR includes actor nodes with typed mailbox, state, handlers, effect summary.
- Codegen produces Erlang gen_server behavior modules (syntax validated but
  runtime testing in Phase 9).
- OD5 scoping decision documented in DECISIONS.md.
- Snapshot tests: actor declarations, typed spawn/send/request, state
  encapsulation violations, exhaustiveness failures, effect summary mismatches.
- `cargo clippy` and `cargo test` pass for `hird-actors`.


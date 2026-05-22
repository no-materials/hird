---
id: hir-m6ra
status: open
deps: [hir-1fif]
links: [hir-actn]
created: 2026-05-22T21:40:15Z
type: task
priority: 1
assignee: nomaterials
parent: hir-y85q
tags: [phase-7, actors, messaging]
---
# Send, request, and receive exhaustiveness

Implement the messaging primitives (send, request) and enforce exhaustiveness
on actor message handlers.

**Send primitive**:
- send(pid: Pid<Msg>, msg: Msg) -> () ! {Send<Msg>}
- Type-checked: msg must match the Pid's message type.
- Fire-and-forget: no return value beyond unit.
- Effect: Send<Msg> (per-process, local — the sender's effect row records that
  it sent, not what the receiver does).

**Request primitive**:
- request(pid: Pid<Msg>, msg_fn: ReplyTo<T> -> Msg) -> T ! {Send<Msg>, Await<T>}
- Constructs a message with an embedded ReplyTo<T>, sends it, and awaits the reply.
- Type-checked: msg_fn must produce a value of the Pid's message type, and the
  reply channel must match the expected return type T.
- Effects: Send<Msg> for the send, Await<T> for the blocking wait.
- Timeout handling: design decision — default timeout? configurable? For v0.1,
  a configurable timeout with a sensible default (5000ms following OTP convention).

**Receive exhaustiveness**:
- An actor's message handlers must cover all constructors of its message type.
- Missing a handler for a message constructor is a compile error: "actor Planner
  does not handle message variant Shutdown."
- This reuses the exhaustiveness checking from Phase 3 but applied at the actor
  declaration level rather than a match expression.

**Effect tracking for sends** (resolves OD8):
- Send<Msg> is a simple effect, not parameterized by the recipient Pid.
  (The Pid is a runtime value; tracking it in the effect type is not practical
  for v0.1.)
- Request<Msg, Reply> combines Send<Msg> and Await<Reply>.
- The transitive effect closure (what does the recipient do?) is a tooling
  query (Phase 10), not a type-system feature.

This ticket resolves **OD8 (Send/reply effect tracking)**.

## Acceptance Criteria

- send type-checks: Pid<Msg> must match message type.
- request type-checks: reply type matches, message constructor matches.
- send has {Send<Msg>} effect; request has {Send<Msg>, Await<T>} effects.
- Missing message handler is a compile error listing unhandled constructors.
- OD8 documented in DECISIONS.md.
- Snapshot tests: valid send, send type mismatch, valid request, request type
  mismatch, missing handler error, complete handler set.
- At least 8 snapshot tests.


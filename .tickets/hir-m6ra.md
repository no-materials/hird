---
id: hir-m6ra
status: closed
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

Implement the messaging primitives (send, request, reply) and enforce
exhaustiveness on actor message handlers.

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
- Timeout handling: fixed 5000ms in v0.1, no surface syntax (see Decisions).

**Reply primitive**:
- reply(reply_to: ReplyTo<T>, value: T) -> () ! {Send<T>}
- A distinct keyword primitive, not an overload of send (see Decisions).
- ReplyTo<T> is consumable only by reply; it has no other operations.

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
- request carries the two separate effects Send<Msg> and Await<Reply>; there
  is no combined Request effect head (the send and the blocking wait have
  different concurrency implications and stay distinct).
- The transitive effect closure (what does the recipient do?) is a tooling
  query (Phase 10), not a type-system feature.

This ticket resolves **OD8 (Send/reply effect tracking)**.

## Decisions (pre-implementation)

Documented as ADR-019 in DECISIONS.md; summary:

1. **OD8 resolved as proposed.** Send<Msg> and Await<T> are separate simple
   effects, not parameterized by the recipient, with no combined Request
   effect head. Transitive closure stays a tooling query (ADR-005).

2. **`reply` is a fourth keyword primitive.** The original spec gave handlers
   no way to answer on a ReplyTo<T>: send is typed over Pid<Msg>, and
   hir-1fif locked ReplyTo as a distinct type, not a Pid alias. Rather than
   overload send over both reference types, reply(reply_to, value) is its own
   keyword form: it preserves the exactly-once (linearity) upgrade path for
   the session-type layer reserved by ADR-018, and maps 1:1 onto
   gen_server:reply/2 vs cast in codegen. Effect is plain Send<T> — no new
   effect head, and the phrasebook's GetStatus handler row stays valid.

3. **request timeout is fixed at 5000ms in v0.1** with no surface syntax.
   Timeout exits the caller (OTP gen_server:call semantics) instead of
   raising a typed error, so request's row stays {Send<Msg>, Await<T>} with
   no Exn; crashes are supervision's job (Phase 8). Extension point if ever
   needed: an optional trailing argument to request.

4. **Exhaustiveness is a set difference, not the Phase 3 matrix.** hir-1fif
   already guarantees each handler names exactly one known constructor with
   no duplicates, so missing-handler detection is a set difference over the
   message type's constructors. The Phase 3 usefulness machinery still checks
   the payload patterns inside each handler, as before.

## Acceptance Criteria

- send type-checks: Pid<Msg> must match message type.
- request type-checks: reply type matches, message constructor matches.
- reply type-checks: value must match the ReplyTo's type parameter.
- send has {Send<Msg>} effect; request has {Send<Msg>, Await<T>} effects;
  reply has {Send<T>} effect.
- Missing message handler is a compile error listing unhandled constructors.
- OD8 documented in DECISIONS.md.
- Snapshot tests: valid send, send type mismatch, valid request, request type
  mismatch, valid reply, reply type mismatch, missing handler error, complete
  handler set.
- At least 8 snapshot tests.


## Notes

**2026-07-07T11:31:19Z**

Implemented: send/request/reply keyword forms parse, check, and lower end to end. send types () ! {Send<Msg>} against Pid<Msg>; request types T ! {Send<Msg>, Await<T>} through a ReplyTo<T> -> Msg builder (constructor); reply is the sole operation on ReplyTo<T> with plain Send<T>. Missing-handler detection is a set difference over the message type's constructors (C0041), listing unhandled variants in declaration order. IR gains Send/Request/Reply nodes; pretty-printer re-emits the surface forms and the round-trip property covers them. 11 new checker snapshot tests plus parser, AST, lowering, and round-trip coverage. OD8 resolved in DECISIONS.md ADR-019.

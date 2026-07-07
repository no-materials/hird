---
id: ha-8fyg
status: open
deps: []
links: []
created: 2026-07-07T12:21:45Z
type: task
priority: 1
assignee: nomaterials
parent: hir-y85q
tags: [phase-7, actors, codegen, design]
---
# Actor-to-Erlang mapping design (ADR)

Lock the actor-to-Erlang gen_server mapping as an ADR in DECISIONS.md. Design only — no emission code; implementation happens in Phase 9 once the expression emitter (hir-zp13) exists.

**Decisions to lock**:

1. **ReplyTo<T> runtime representation** (deferred to codegen by ADR-018).
   Proposed: ReplyTo<T> is the gen_server From term. It is erased from the
   wire message; the handler binds it from handle_call's second argument,
   and reply lowers to gen_server:reply(From, Value).

2. **call/cast dispatch rule**. Which constructors arrive via handle_call
   vs handle_cast, decided per constructor by whether it carries a
   ReplyTo field. Address the forwarding edge case: send-ing a
   ReplyTo-carrying message would give the same constructor two wire
   shapes (cast with embedded From vs call with envelope From) — forbid,
   or always embed; pick one.

3. **request argument restriction**. request takes a message-building
   function ReplyTo<T> -> Msg; if arbitrary lambdas are allowed, codegen
   cannot statically strip the ReplyTo field to build the
   gen_server:call payload. Proposed: restrict to bare constructors.
   Also decide: zero/multiple ReplyTo fields per constructor, nested
   ReplyTo.

4. **Module layout and naming**. One .erl module per actor; actor name to
   module name mangling; Hird init/handler function naming within the
   module; collision policy with Erlang reserved words and stdlib module
   names.

5. **Out of scope**: effect-handler parameter threading (ADR-013) across
   the spawn boundary — how a threaded tool handler reaches a spawned
   gen_server — is a Phase 9 concern; state that explicitly in the ADR.

## Acceptance Criteria

- ADR added to DECISIONS.md covering the four decision areas above.
- Explicit out-of-scope note for handler threading across spawn.
- hir-1dvq (Phase 9 implementation) is consistent with the locked mapping.
- No emission code in this ticket.


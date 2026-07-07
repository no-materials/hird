---
id: hc-uz3r
status: open
deps: []
links: []
created: 2026-07-07T13:40:49Z
type: task
priority: 1
assignee: nomaterials
parent: hir-y85q
tags: [phase-7, actors, check]
---
# Enforce ReplyTo wire restrictions from ADR-020

ADR-020 decision 3 restricts where ReplyTo may appear so every message
constructor has exactly one wire shape. hir-m6ra's checker predates it:
infer_request unifies the builder against ReplyTo<T> -> Msg but accepts
any expression, and nothing forbids applying a ReplyTo-carrying
constructor outside request position.

**Checks to add** (hird-check):

1. request's message-builder must be a bare constructor of the target
   mailbox type — a syntactic check before type unification; arbitrary
   lambdas are a dedicated diagnostic.
2. A constructor with a ReplyTo field is applicable only as request's
   builder; ordinary application (and therefore send payloads) is a
   compile error.
3. In a message sum type, ReplyTo<t> may appear only as a direct field
   of a constructor, at most once per constructor; nested occurrences
   (including through named type references) are errors. ReplyTo in
   actor state types stays legal (deferred reply).

## Decisions (pre-implementation)

Decision 1 amends ADR-020 in DECISIONS.md; 2 and 3 are checker scope
clarifications locked here.

1. **A ReplyTo field must be the constructor's only field.** With the
   builder restricted to a bare constructor reference, a constructor
   carrying payload alongside ReplyTo (e.g. `Query(String, ReplyTo<R>)`)
   could never be applied anywhere: bare, it does not unify with
   `ReplyTo<T> -> Msg`, and check 2 bans ordinary application. Rejecting
   it at declaration with a dedicated diagnostic replaces a dead
   declaration plus a confusing unification failure at the request site.
   Payload-carrying requests are inexpressible in v0.1; admitting them
   later (partial application, or a record payload) is additive.
   Consequence for Phase 9: every gen_server:call payload is a bare
   constructor atom.

2. **Check 3 is mailbox-scoped, run at the actor declaration.** It
   cannot be a blanket rule on type declarations: message types are
   standalone `type` declarations, and deferred-reply state
   legitimately nests ReplyTo (e.g. `Option<ReplyTo<T>>` in a state
   record). The walk resolves named type references
   instantiation-aware and cycle-safe. Diagnostics anchor at the
   actor's message type, naming the offending constructor and nesting
   path. A ReplyTo-carrying sum never used as a mailbox stays legal;
   its call constructors are simply unusable (no dedicated
   diagnostic).

3. **Check 2 restricts value/application position only.** Constructor
   patterns stay legal — handlers must match call constructors. No
   whole-message forwarding loophole exists: handler patterns are
   already forced to top-level constructor form, so a handler can
   never rebind a full call message and send it.

## Acceptance Criteria

- Bare-constructor restriction on request enforced with a dedicated diagnostic.
- Applying a ReplyTo-carrying constructor outside request position is a compile error.
- Message type declarations reject nested or repeated ReplyTo fields.
- A ReplyTo-carrying constructor with additional payload fields is
  rejected at the actor declaration with a dedicated diagnostic.
- ReplyTo in state types still accepted.
- Snapshot tests for each new diagnostic and for the still-legal cases.
- cargo clippy and cargo test pass.


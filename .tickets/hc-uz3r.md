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

## Acceptance Criteria

- Bare-constructor restriction on request enforced with a dedicated diagnostic.
- Applying a ReplyTo-carrying constructor outside request position is a compile error.
- Message type declarations reject nested or repeated ReplyTo fields.
- ReplyTo in state types still accepted.
- Snapshot tests for each new diagnostic and for the still-legal cases.
- cargo clippy and cargo test pass.


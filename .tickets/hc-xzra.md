---
id: hc-xzra
status: open
deps: []
links: []
created: 2026-07-30T13:17:55Z
type: task
priority: 2
assignee: nomaterials
tags: [codegen, backend, v0.2]
---
# v0.2 backend stage: emit Erlang abstract forms

ADR-002 stages the backend deliberately: v0.1 emits readable Erlang
source (shipped), v0.2 hands abstract forms to `compile:forms`, and
v0.3+ targets Core Erlang. This is the v0.2 step.

Implement an abstract-forms emitter over the same IR contract, beside
the source emitter rather than replacing it: the source backend stays
the default (and the debugging surface) until the forms backend
reaches parity on the conformance suite and demos.

## Design

- Target the erl_parse abstract format (what `compile:forms/2`
  consumes); the existing lowering and the source emitter's structure
  (handler-map threading, tool dispatch, per-constructor dispatch —
  ADR-020/022) map one-to-one.
- Decide the handoff: serialize forms to a term file consumed by a
  small runtime driver that calls `compile:forms`, or drive erlc-less
  compilation from the build pipeline directly.
- Line numbers come from IrSpan, replacing the `%% <file>:<line>`
  comment convention with real annotations.
- Backend selection is a build-pipeline flag; ADR-002's consequences
  section already anticipates the seam.

## Acceptance Criteria

- The build pipeline can produce .beam for both demo programs via
  abstract forms, selected by a flag.
- The conformance suite and demo end-to-end tests pass under the forms
  backend.
- The source backend remains the default and untouched in behavior.
- cargo clippy and cargo test pass workspace-wide.


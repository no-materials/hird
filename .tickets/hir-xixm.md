---
id: hir-xixm
status: open
deps: []
links: []
created: 2026-07-28T07:58:56Z
type: epic
priority: 0
assignee: nomaterials
tags: [supervision, codegen, demo]
---
# Supervisor runtime surface: supervise and child forms

## Goal

Close the gap the v0.1 demo exposed: no Hirð expression starts a declared
supervisor, and no expression reaches a supervised child, so `hird run`
cannot drive an actor that actually runs under supervision. The flagship
demo spawns its planner directly; PlannerSup is checked, emitted, and in
the effect graph, but runtime-dead. Add two keyword forms — `supervise`
to start a declared supervisor and `child` for typed child lookup — so
the demo's planner genuinely runs under PlannerSup from `hird run` alone.

## Publish relevance

The v0.1 pitch makes supervisor boundaries part of the headline claim. A
demo whose supervisor is never started at runtime is the first thing a
skeptical reader will find. This epic gates publishing.

## Design context

What exists already, and what the gap is:

- `spawn` resolves actor names only (ADR-018's actor namespace);
  `spawn(PlannerSup)` is a compile error.
- Implicit start-all-supervisors at boot was explicitly rejected on
  hir-y9jo (explicit over implicit).
- Emitted supervisors register `{local, Module}` and their children stay
  unregistered (ADR-020, hir-z9rn), so even a started tree's children are
  unreachable from Hirð: `send`/`request` need a `Pid<Msg>` and nothing
  produces one.
- `hird_sup_util:child_pid/2` shipped in hir-7oph for exactly this lookup
  and has no consumer.
- Supervised crash-restart was verified on BEAM only at the
  runtime-library level (hir-7oph), never from a Hirð program.

The design is locked first as an ADR (it sits on top of ADR-018/020/021),
then implemented, then the demo moves onto it.

## Task sequence

1. [x] [hir-x5gc](hir-x5gc.md) — Lock the supervisor runtime surface ADR
2. [ ] [hir-ugi0](hir-ugi0.md) — Implement supervise and child keyword forms
3. [ ] [hir-r4d1](hir-r4d1.md) — Run the demo planner under PlannerSup

## Out of scope

- First-class supervisor values (`SupRef`) — the registered name is the
  handle, consistent with ADR-018.
- Deterministic in-program observation of a restart: any probe races the
  crash (a request enqueued behind a poison message exits the caller; a
  fresh child lookup can return the pre-crash pid). Needs monitor/await
  surface or dispatcher-audited crash records — future design.
- Dynamic children, nested supervision trees, restart-policy surface
  changes.

## Acceptance Criteria

- The ADR is locked in DECISIONS.md.
- `supervise(PlannerSup)` and `child(PlannerSup, planner)` check, lower,
  and emit; erlc-validated and verified on BEAM.
- `hird run demo/agent_planner.hird` drives a planner that is running as
  a supervised child of PlannerSup.
- The dry-run harness still passes against the supervised demo.
- README and phrasebook document the forms.
- cargo fmt, clippy -D warnings, and workspace tests pass.


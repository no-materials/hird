---
id: hir-ugi0
status: open
deps: [hir-x5gc]
links: []
created: 2026-07-28T07:59:04Z
type: task
priority: 0
assignee: nomaterials
parent: hir-xixm
tags: [supervision, codegen]
---
# Implement supervise and child keyword forms

Implement the supervisor runtime surface ADR (hir-x5gc) end to end, the
hir-shiv pattern: each pipeline stage gains a small form that reuses the
adjacent machinery rather than duplicating it.

Scope:
- Lexer: `supervise` and `child` keywords.
- Parser: `supervise(SupName)` and `child(SupName, child_id)` keyword
  forms, mirroring `spawn`'s grammar (name in a declaration namespace,
  parenthesised arguments).
- Checker: `supervise` resolves the supervisor namespace (dedicated
  error for unknown names, like spawn's C0039), types as `()`, and
  contributes the checker-known bare `Supervise` effect. `child`
  additionally checks the child id against the named supervisor's
  declared children and types as `Pid<Msg>` from the child actor's
  message type, with the empty row.
- IR + lowering: dedicated nodes beside IrSpawn, spans per ADR-022 §4.
- Codegen: `supervise` emits the supervisor module's `start_link/0`
  under a `{ok, _}` match evaluating to unit; `child` emits the
  `hird_sup_util:child_pid/2` lookup with a crash on `error`
  (`{no_child, Id}`) — inline case or a runtime helper, emitter's
  choice.
- Effect graph: nothing new required (supervisors are already
  projected); confirm the `Supervise` head renders sensibly in rows.

Out of scope: demo changes (hir-r4d1), first-class supervisor values,
restart observation surface.

## Acceptance Criteria

- Both keywords lex; both forms parse, with parser coverage alongside
  spawn's.
- Checker: unknown supervisor and unknown child id are dedicated
  diagnostics; `child` on a declared pair types as the actor's
  `Pid<Msg>`; `supervise` contributes `Supervise` to the row and a main
  that installs, supervises, and looks up checks clean.
- Emitted Erlang erlc-validates; snapshot coverage in hird-codegen.
- End-to-end on BEAM: supervise a declared supervisor, look up its
  child, send/request against the returned pid, and see the child's
  tool-call audit records — from `hird run` alone.
- cargo fmt, clippy -D warnings, and workspace tests pass.


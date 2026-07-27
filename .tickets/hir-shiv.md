---
id: hir-shiv
status: open
deps: []
links: []
created: 2026-07-27T12:26:53Z
type: task
priority: 0
assignee: nomaterials
parent: hir-7rsf
tags: [phase-9, lang, codegen]
---
# install blocks: dynamic-extent registry handlers from Hirð

Implement ADR-023's `install { … } in e` expression form end to end, so a
Hirð program can supply the registry defaults that spawned actors' tool
calls resolve through (ADR-020 §6, ADR-022 §3) without an Erlang sidecar
module.

Scope:
- Lexer: `install` keyword.
- Parser: same arm grammar as `handle`, followed by `in <expr>`.
- Checker: arms checked exactly like handle arms (ADR-013 structural,
  ADR-017 signature-directed for Tool<…>); new requirement that each
  handler's effect row is closed and empty, with its own diagnostic code;
  the expression's row is body ∪ {Install}, where Install is a
  checker-known bare head like Spawn/Send/Await (no user declaration).
- IR + lowering: a dedicated node alongside the handle node, spans per
  ADR-022 §4.
- Codegen: lower to hird_handlers:with_handlers(Entries, fun() -> Body
  end), reusing ADR-022 §1's key scheme and binary-fun arm normalisation.
- phrasebook.md: document the form next to Handle Blocks.

Out of scope: relaxing the pure-handler restriction, per-key Install
granularity, uninstall surface, any CLI change (hird run already permits
residual non-Tool effects in main).

## Acceptance Criteria

- `install` keyword lexes; the form parses with handle's arm grammar.
- Arm checking matches handle blocks; a handler with a non-empty or open
  row is rejected with a dedicated diagnostic.
- Install appears in the expression's inferred row; `hird check` accepts
  a main that installs then spawns.
- Emitted Erlang calls hird_handlers:with_handlers with correctly keyed,
  normalised entries; erlc-validated.
- End-to-end on BEAM: a spawned actor's tool call resolves to a handler
  installed by main's install block, produces its audit record, and the
  registry is restored after the body (crash included).
- phrasebook.md documents the form.
- cargo fmt, clippy -D warnings, and workspace tests pass.


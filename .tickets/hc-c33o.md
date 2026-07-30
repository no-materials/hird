---
id: hc-c33o
status: open
deps: []
links: []
created: 2026-07-30T13:17:42Z
type: task
priority: 2
assignee: nomaterials
tags: [checker, tooling, mcp]
---
# Record per-expression effect rows in the checker

The checker records effect rows only at handle/install blocks and
actor/supervisor declarations; `CheckedFile::effect_rows` has no
entries for ordinary expressions. Downstream, the MCP server's
infer_type (and LSP hover) can report a row only when the expression's
*type* is a function type — at a call site like
`create_ticket({ ... })` the tool answers `{}` even though evaluating
the expression performs `Tool<CreateTicket>`.

Record the resolved effect row of every effectful expression node
during inference (NodeKey → EffectRow), so location-based queries can
answer "what does evaluating this perform" rather than "what row does
this value's type carry".

## Design

- Rows must be resolved against the final substitution; either record
  through a deferred list resolved at check end (the existing
  `effect_rows` Vec already does this) or resolve eagerly with care.
- Start with the nodes where a row is introduced: application, send,
  request, reply, spawn, supervise — not every literal; keep the table
  proportional to effectful nodes.
- `effect_row_at` already walks the table; the MCP/LSP side is then a
  small change (consult ancestors for the nearest recorded row).

## Acceptance Criteria

- `effect_row_at` returns the performed row for application and
  messaging expressions in a checked file.
- MCP infer_type at a tool-call site reports the tool's effect (e.g.
  `{Tool<CreateTicket>}`), not `{}`.
- Tests: checker unit tests for recorded rows; an MCP integration test
  covering the call-site case.
- cargo clippy and cargo test pass workspace-wide.


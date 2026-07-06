---
id: hi-gggf
status: closed
deps: []
links: []
created: 2026-07-06T14:24:42Z
type: task
priority: 2
assignee: nomaterials
tags: [ir, tools, phase-9]
---
# Tool declarations do not lower to IR; tool-handling programs cannot round-trip

Phase 6's signature-directed handler checking surfaced an IR gap: tool
declarations are dropped during lowering (no IR declaration kind carries
them), so pretty-printing a module that handles a tool effect emits a
program with a `Tool<Marker>` handle arm but no backing `tool` declaration.
Re-checking that output fails with C0033 ("not a declared tool") — the
round-trip fixtures were switched from `Tool<Repo>` to a non-tool
parametric effect head (`Db<Repo>`) to keep them exercising
parametric-effect printing.

The gap matters beyond round-trip fidelity: the Phase 9 Erlang backend
consumes the IR, and lowering a tool call site or a tool handle arm needs
the tool's signature (marker, args record, result, trailing row) available
in the module IR rather than only in checker side-tables
(`tool_signatures`, `invocation_records`).

## Acceptance Criteria

- Tool declarations lower to an IR declaration carrying name, type
  parameters, args/result types, and trailing row.
- The IR pretty-printer re-emits tool declarations, and a round-trip test
  over a handle block handling a declared tool passes (recheck is clean
  under signature-directed handler checking).
- JSON serialisation covers the new declaration kind; snapshot tests
  updated/added.
- `cargo fmt`/`clippy`/`test` pass.

## Notes

**2026-07-06T14:38:22Z**

Implemented in commit 07251f9. Tool declarations lower to IrToolDef (name, params, input, output, trailing row without the implicit Tool<name>), derived from the checker-recorded function scheme with quantified variables renamed to the declared parameter names. The pretty-printer emits the surface tool form and synthesises the implied Tool effect declaration, so tool-handling programs round-trip: the fixtures are back on Tool<Repo> handle arms (no C0033 on recheck, no Db<Repo> workaround), with a generic-tool round-trip added. JSON covers the new kind (snapshot updated); docs/ir.md documents IrToolDef and the previously undocumented IrHandle. fmt/clippy/test clean across the workspace.

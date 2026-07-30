---
id: hl-et6u
status: open
deps: []
links: []
created: 2026-07-30T13:17:16Z
type: task
priority: 1
assignee: nomaterials
tags: [tooling, lsp, mcp]
---
# Cross-module analysis for the LSP and MCP servers

Both hird-lsp and hird-mcp compile each file as a single-module
program, so `use` imports never resolve: the LSP reports imported
names as unresolved and go-to-definition stops at the file boundary,
and every MCP tool answers as if the file's imports did not exist.
This is the biggest real limitation in the tooling.

The checker already supports whole programs — `check_program` takes a
list of modules, and the CLI compiles directories that way. The
servers just never feed it more than one file.

Teach both servers to discover sibling modules and compile them as one
program:

- Module discovery mirrors the CLI's directory loading: the `.hird`
  siblings of the queried/open file, with file-stem-derived module
  names (`pipeline::load` in hird-cli is the reference).
- The LSP compiles the open document's directory as a program and
  answers hover/definition/diagnostics from the whole-program tables;
  definitions may resolve into files that are not open.
- The MCP cache becomes program-scoped: one cache entry per directory,
  invalidated when any member file's source text changes.

## Acceptance Criteria

- LSP: hover and go-to-definition resolve names imported with `use`
  from a sibling module, with locations in the defining file.
- LSP: diagnostics no longer flag resolvable imports as unresolved.
- MCP: lookup_definition, infer_type, render_ir_fragment, and
  get_context_for_symbol answer for imported symbols; the response
  carries the defining file.
- Two-module fixtures with tests in both crates.
- cargo clippy and cargo test pass workspace-wide.


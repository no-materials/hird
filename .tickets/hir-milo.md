---
id: hir-milo
status: open
deps: []
links: []
created: 2026-05-22T21:42:38Z
type: task
priority: 2
assignee: nomaterials
parent: hir-9sjy
tags: [phase-10, lsp]
---
# LSP scaffold with tower-lsp

Implement a basic LSP server in hird-lsp using tower-lsp.

**v0.1 scope** (minimal viable LSP):

1. **Diagnostics on save**: when a file is saved, run the Hirð type checker and
   publish diagnostics (errors and warnings) with source spans.

2. **Hover for type info**: hovering over an identifier shows its inferred type
   and effect row. Format: "x : Int" or "read_file : Path -> String ! {Tool<ReadFile>}".

3. **Go-to-definition**: jump to the definition of a function, type, actor,
   or effect.

**Implementation**:
- Use tower-lsp for the LSP protocol handling.
- Reuse the compiler pipeline for parsing, type inference, and IR construction.
- Cache compilation results per file; invalidate on change.
- No incremental compilation (salsa) yet — full recompile per file on change.

**Out of scope for v0.1**:
- Completion (autocomplete).
- Rename/refactor.
- Code actions (quick fixes).
- Workspace-wide analysis.
- Incremental compilation.

These are real limitations. Document them in the LSP section of the README so
users know what to expect.

## Acceptance Criteria

- hird-lsp binary runs as an LSP server.
- Diagnostics published on file save with correct source spans.
- Hover shows inferred type for identifiers.
- Go-to-definition works for functions, types, actors, effects.
- At least one integration test confirming LSP responses for a simple Hirð file.
- README documents LSP capabilities and known limitations.


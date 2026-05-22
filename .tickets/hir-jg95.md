---
id: hir-jg95
status: open
deps: []
links: []
created: 2026-05-22T21:35:51Z
type: task
priority: 2
assignee: nomaterials
parent: hir-b9gf
tags: [phase-0, docs]
---
# Project documentation skeletons

Create the project documentation framework:

- `CONTRIBUTING.md`: how to build, test, run CI locally, naming conventions,
  commit style, crate responsibilities overview.
- `ARCHITECTURE.md`: one section per compiler crate explaining its responsibility,
  its inputs and outputs, and what it depends on. Diagram showing the compilation
  pipeline: source -> lex -> parse -> AST -> types -> effects -> IR -> codegen -> .erl.
- `DECISIONS.md` (ADR-style): locked architecture decisions from the handoff.
  Each decision as a dated entry with context, decision, consequences, and status.

Locked decisions to document:
1. Rust compiler (not self-hosted).
2. Staged backend: Erlang source in v0.1, abstract forms in v0.2, Core Erlang v0.3+.
3. OTP for supervision (not a custom runtime).
4. DI-style handlers in v0.1 (no CPS/delimited control).
5. Per-process effect semantics (not transitive across messages).
6. Opaque-capability discipline for stateful resources.
7. Unicode canonicalization at the lexer.
8. MSRV 1.92, edition 2024.

## Acceptance Criteria

- CONTRIBUTING.md exists with build/test/style instructions.
- ARCHITECTURE.md exists with crate-by-crate responsibility map.
- DECISIONS.md exists with all 8 locked decisions as ADR entries.
- All docs are valid markdown, pass any configured linters.


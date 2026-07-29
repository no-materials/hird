---
id: hir-9sjy
status: open
deps: [hir-0rzf, hir-7rsf]
links: []
created: 2026-05-22T21:35:14Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-10, llm, tooling, mcp, lsp]
---
# Phase 10 — LLM Tooling

## Goal

Cash out the "LLM-first" design claim in concrete tooling: an MCP server for
compiler introspection, a finalized phrasebook for LLM context windows, and
an LSP scaffold for editor support.

## v0.1 demo relevance

The MCP server answers "what does the Planner actor do?" by returning the
structured effect summary, message protocol, and supervisor relationship from
the IR. This is the demo's tooling headline: the compiler is queryable
by LLM agents, not just by humans reading source code.

## Design context

**MCP server** (modeled on a sibling project's stdio MCP server):
Tools exposed:
- `infer_type(expr)` — return the inferred type of an expression in context.
- `lookup_definition(name)` — return the source location and type of a definition.
- `explain_effect_row(fn_name)` — return the effect row of a function in readable form.
- `render_ir_fragment(name)` — return the IR JSON for a definition.
- `explain_actor_protocol(actor_name)` — return the actor's message type, state type,
  handler signatures, and effect summary.
- `emit_actor_effect_graph(actor_name)` — return the full actor/effect graph rooted
  at the named actor, including supervisor relationships and transitive tool effects.
- `get_context_for_symbol(name)` — return a context-budget-aware summary of a symbol
  suitable for inclusion in an LLM prompt.
- `get_context_budget()` — return how much context (in tokens, approximately) the
  current project consumes.

**phrasebook.md**: a dense reference document covering canonical patterns, common
pitfalls, type system quirks, tool effect usage, supervisor patterns, naming
conventions, and Unicode operator forms. Designed for inclusion in LLM context
windows. This is not a tutorial — it's a reference sheet.

**LSP scaffold**: basic Language Server Protocol support using `tower-lsp`.
v0.1 scope: go-to-definition, hover for type info, diagnostics on save.
Full completion, refactoring, and code actions are deferred.

**Documentation split**: separate docs for "writing Hirð as a human" and
"writing Hirð as an LLM agent." The LLM-targeted doc focuses on constraints
the LLM must respect (canonical naming, effect declarations, no ambient state),
the phrasebook, and the MCP tools available.

## Task sequence

1. [x] [hir-126o](hir-126o.md) — Phrasebook and split documentation
2. [x] [hir-milo](hir-milo.md) — LSP scaffold with tower-lsp
3. [ ] [hir-q0as](hir-q0as.md) — MCP server for compiler introspection

Steps 1 and 2 have no internal deps. Step 3 requires the v0.1 demo
(hir-bxdd from Phase 9).

## Out of scope

- Full LSP completion, refactoring, or code actions (deferred).
- MCP server performance optimization (v0.1 targets correctness).
- LLM fine-tuning or training data generation.
- Editor-specific plugins beyond LSP (VS Code extension, etc.).

## Acceptance Criteria

- MCP server binary exposable via stdio or HTTP with tools: infer_type,
  lookup_definition, explain_effect_row, render_ir_fragment,
  explain_actor_protocol, emit_actor_effect_graph, get_context_for_symbol,
  get_context_budget.
- MCP server returns correct results for the v0.1 planner demo: querying
  the Planner actor returns its effect summary, message protocol, and supervisor.
- `phrasebook.md` finalized with substantive content for all sections.
- LSP scaffold with tower-lsp: go-to-definition, hover type info, diagnostics
  on save. At least one test confirming LSP responses for a simple Hirð file.
- `docs/writing-hird-human.md` and `docs/writing-hird-llm.md` exist with
  substantive content.
- `cargo clippy` and `cargo test` pass for `hird-mcp` and `hird-lsp`.


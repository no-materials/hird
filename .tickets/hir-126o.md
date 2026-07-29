---
id: hir-126o
status: closed
deps: []
links: []
created: 2026-05-22T21:42:28Z
type: task
priority: 2
assignee: nomaterials
parent: hir-9sjy
tags: [phase-10, docs, llm]
---
# Phrasebook and split documentation

Finalize phrasebook.md and write the split documentation: one doc for human
authors, one for LLM agents.

**phrasebook.md** — dense LLM-context reference:
- Canonical naming rules (snake_case values, PascalCase types, single-char tyvars).
- Unicode operator forms and their ASCII equivalents.
- Function declaration patterns (with and without effect annotations).
- ADT declaration patterns.
- Pattern matching patterns (exhaustive match, nested patterns).
- Actor declaration patterns (message type, handlers, effect summary).
- Supervisor declaration patterns.
- Tool effect declaration and handler patterns.
- Effect row syntax (closed, open, empty).
- Handle block patterns (mocking, dry-run, log redirection).
- Common pitfalls (non-exhaustive match, missing effect annotation, opaque type
  destructuring outside module).
- Capability discipline examples (Table, Tool, Log, Clock).

This document is designed to be included wholesale in an LLM context window.
It should be under 4000 tokens and cover the 80% most common patterns.

**docs/writing-hird-human.md** — tutorial-style guide for human developers:
- Getting started (install, hello world, build, run).
- Language tour (types, functions, effects, actors, supervisors, tools).
- The effect system explained with examples.
- The error model (errors vs crashes).
- Working with the CLI.

**docs/writing-hird-llm.md** — constraints and reference for LLM agents:
- Naming rules the LLM must follow (enforced by compiler).
- Effect annotation requirements.
- Capability discipline (no ambient state).
- How to use MCP tools to query the compiler.
- Common mistakes LLMs make and how the compiler catches them.
- The phrasebook as a quick reference.

## Acceptance Criteria

- phrasebook.md finalized with all sections, under 4000 tokens.
- docs/writing-hird-human.md exists with getting started + language tour.
- docs/writing-hird-llm.md exists with constraints, MCP tool usage, pitfalls.
- All docs are valid markdown.
- phrasebook.md reviewed for accuracy against the implemented compiler.


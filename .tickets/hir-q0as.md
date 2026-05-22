---
id: hir-q0as
status: open
deps: [hir-bxdd]
links: []
created: 2026-05-22T21:42:14Z
type: task
priority: 1
assignee: nomaterials
parent: hir-9sjy
tags: [phase-10, mcp, llm]
---
# MCP server for compiler introspection

Implement the MCP (Model Context Protocol) server in hird-mcp that exposes
compiler introspection tools to LLM agents.

**Server setup**: stdio-based MCP server (modeled on a sibling
project's stdio MCP server). Runs as a subprocess launched by an LLM
agent framework or IDE.

**Tools exposed**:

1. `infer_type(file, expr_location)` — return the inferred type of an expression
   at a given source location. Response: { type: "Option<Int>", effect_row: "{}" }.

2. `lookup_definition(file, name)` — return the source location, type, and
   documentation of a definition. Response: { file, line, type, doc, kind }.

3. `explain_effect_row(file, fn_name)` — return the effect row of a function in
   human-readable form with explanations of each effect.

4. `render_ir_fragment(file, name)` — return the Glass IR JSON for a definition.

5. `explain_actor_protocol(file, actor_name)` — return the actor's message type
   constructors, state type, handler signatures, and effect summary.

6. `emit_actor_effect_graph(file, actor_name)` — return the full actor/effect
   graph rooted at the named actor, including supervisor relationships and
   transitive tool effects.

7. `get_context_for_symbol(file, name, budget)` — return a context-budget-aware
   summary of a symbol: its type, its effect row, its callers, its callees.
   Fits within the specified token budget.

8. `get_context_budget(file)` — return approximate token counts for types,
   effects, actors, and tools in the file.

**Implementation**: the MCP server reuses the compiler pipeline (parse, infer,
lower to IR) and queries the IR for responses. Compilation is lazy — only
compile on first query, cache results.

## Acceptance Criteria

- MCP server binary runs via stdio.
- All 8 tools respond correctly for the v0.1 planner demo.
- explain_actor_protocol for Planner returns message type, handlers, effect summary.
- emit_actor_effect_graph for Planner includes supervisor and tool effects.
- get_context_for_symbol returns budget-constrained summaries.
- Error handling: invalid file, undefined name, parse errors all return
  structured error responses (not crashes).
- At least one integration test per MCP tool.


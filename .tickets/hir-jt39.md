---
id: hir-jt39
status: closed
deps: [hir-rlo4]
links: []
created: 2026-05-22T21:33:43Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-6, tools, effects]
---
# Phase 6 — Tool Effects

## Goal

Implement tool effects as a typed primitive: a special class of effect
representing auditable, structured invocations of external (often
non-deterministic, often LLM-mediated) tools. This is the language's most
novel concept.

## v0.1 demo relevance

The planner demo uses Tool<ReadRepo> and Tool<CreateTicket> as its primary
effects. The audit log showing every tool invocation with structured arguments
and return values is the demo's headline output. Without tool effects, the demo
is just another typed actor system.

## Design context

A tool effect differs from a regular effect in two ways:

1. **Structured invocation record.** Every tool call produces a complete,
   typed record of: tool name, structured arguments, structured return value,
   timestamp, caller identity. The compiler generates the record type from the
   tool declaration.

2. **Audit destination.** The tool effect type carries information about where
   its invocation records go. The audit log is a first-class language concept.

Tool declarations in surface syntax:
```
tool ReadRepo : { path: Path } -> RepoState
tool CreateTicket : { title: String, body: String } -> TicketId
tool LLMCall<T> : { prompt: Prompt, schema: Schema<T> } -> T
```

Standard library tools for v0.1: `llm_call`, `http_get`, `http_post`,
`read_file`, `write_file`, `shell`. Each as a tool effect with proper
structured types.

## Task sequence

1. [x] [hir-4g3y](hir-4g3y.md) — Tool declarations and invocation records
2. [x] [hir-jgs1](hir-jgs1.md) — Audit log integration and tool effect docs
3. [x] [hir-uvui](hir-uvui.md) — Signature-directed handler checking for tool effects

## Open design questions resolved in this phase

- **OD2 (LLM call typing)**: Resolve how LLM calls are typed. Strong lean toward
  schema-typed: `llm_call<T>(prompt, schema) -> T ! {Tool<LLM>, Exn ParseError}`.
  Document decision.
- **OD3 (Audit log fidelity)**: Resolve initial audit log guarantees. Recommend
  starting at structured-log-with-clear-upgrade-path. Document decision.
- **OD4 (Tool effect replay semantics)**: Resolve whether replay re-executes or
  returns logged values. Recommend logged-values for audit, re-execute for debug.
  Document decision.

## Out of scope

- Content-addressed Merkle-chained audit logs (future work).
- Tool effect sandboxing or permission systems beyond capability passing.
- Real LLM integration (the v0.1 demo uses mock handlers).
- HTTP client implementation (tool effects are abstract; handlers provide impls).

## Acceptance Criteria

- `tool` declaration syntax parsed and type-checked.
- Compiler generates invocation record types for each tool declaration.
- Tool effect rows integrate with the general effect system: Tool<ReadRepo> is
  a valid effect in a function's effect row.
- Standard library tool declarations exist for: llm_call, http_get, http_post,
  read_file, write_file, shell.
- DI-style handlers for tool effects: a handle block can replace tool
  implementations with mocks for testing or dry-run execution.
- Audit log emission: tool invocations produce structured JSON records with tool
  name, arguments, return value, timestamp.
- `docs/tool-effects.md` written: full explanation of tool effect semantics,
  invocation records, handler patterns, audit log format, and comparison with
  regular effects.
- OD2, OD3, and OD4 decisions documented in DECISIONS.md.
- Snapshot tests: tool declarations, tool calls in effect rows, handler
  substitution, invocation record generation, audit log output format.
- `cargo clippy` and `cargo test` pass.


## Notes

**2026-07-02T06:33:22Z**

hir-4g3y (tool declarations and invocation records) closed; landed in commit 79c69ca. OD2 resolved and hir-x6cx closed with it. The tool-declaration mechanism, record types, generated functions, derived invocation records, and the six standard-tool fixture declarations are in. Remaining: hir-jgs1 (audit log + docs, resolves OD3/OD4) and hir-uvui (signature-directed handler checking), both now unblocked.

**2026-07-03T09:45:00Z**

hir-jgs1 (audit log integration and tool effect docs) closed; landed in
commits 36e0aff, bea7971, f388ddf. OD3 and OD4 resolved (ADR-016) and
hir-yum3/hir-v3pv closed with it. The wire format is locked and pinned by
the conformance/v1 golden files, the reference serializer/decoder/replay
live in hird-check::wire, the audit sink is capability-based with positive
and negative fixtures, and docs/tool-effects.md is written. Remaining:
hir-uvui (signature-directed handler checking), the phase's last task.

**2026-07-06T14:07:34Z**

hir-uvui (signature-directed handler checking) closed; landed in commit 6da2ac0. ADR-017 records the decision: handle arms over Tool<Marker> check the handler against the tool's operation signature by instantiate-and-unify from a marker-keyed side-table, with a fresh open row (pure mocks accepted) and the monomorphic-handler-for-generic-tool gap accepted for v0.1. New diagnostics C0033 (not a declared tool) and C0034 (signature mismatch) join the retained structural checks. All three phase-6 tasks are now closed; the epic's acceptance criteria are met.

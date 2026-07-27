---
id: hir-7rsf
status: open
deps: [hir-cnq8]
links: []
created: 2026-05-22T21:34:50Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-9, codegen, runtime, demo]
---
# Phase 9 — Codegen and Runtime

## Goal

Complete the end-to-end compilation pipeline: IR to Erlang source emission,
an Erlang runtime support library, the hird CLI, and — critically — the v0.1
supervised agent planner demo running on BEAM.

## v0.1 demo relevance

This IS the demo phase. Everything prior builds toward this. The deliverable is
a working Hirð program that:
1. Defines a Planner actor that receives a repository path.
2. Reads repo state through Tool<ReadRepo>.
3. Creates tickets through Tool<CreateTicket>.
4. Logs through Log.
5. Runs under a one_for_one supervisor.
6. Has a test harness that installs mock handlers for dry-run testing.
7. Emits a human-readable actor/effect graph as queryable JSON.
8. Compiles to Erlang source that runs on stock BEAM.

## Design context

**Erlang source emission**: the codegen produces `.erl` files from IR.
Generated code must compile with stock `erlc` without modifications. Source
maps are preserved as comments or sidecar files for debugging.

The generated Erlang should be human-readable — not beautifully formatted, but
inspectable. A developer debugging a Hirð program should be able to read the
generated Erlang and understand what the source Hirð does. This is a deliberate
design choice for v0.1 (vs. abstract forms or Core Erlang, which are less
inspectable).

**Runtime support library**: a small, hand-written Erlang library providing:
- Tool effect dispatcher (routes tool calls through handler chain).
- Audit log sink (accepts invocation records, writes structured JSON).
- Default supervisor wiring (standard OTP supervisor module template).
- DI handler installation machinery (process dictionary or explicit state
  threading for handler lookup).

**CLI commands**:
- `hird check` — type-check without codegen.
- `hird build` — compile to Erlang source + compile with erlc.
- `hird run` — build then run on BEAM.
- `hird repl` — interactive evaluation (stretch goal for v0.1; may defer).
- `hird emit-ast` — dump typed AST as JSON.
- `hird emit-effect-graph` — dump actor/effect graph as JSON.

## Task sequence

1. [x] [hir-zp13](hir-zp13.md) — Erlang source emission from IR
2. [x] [hir-1dvq](hir-1dvq.md) — Actor codegen to Erlang gen_server
3. [x] [hir-z9rn](hir-z9rn.md) — Supervisor codegen to Erlang
4. [x] [hir-7oph](hir-7oph.md) — Erlang runtime support library
5. [x] [hir-y9jo](hir-y9jo.md) — CLI commands: check, build, run, emit
6. [x] [hir-shiv](hir-shiv.md) — install blocks: dynamic-extent registry handlers from Hirð
7. [ ] [hir-bxdd](hir-bxdd.md) — v0.1 demo: supervised agent planner end-to-end

Step 2 implements the mapping locked by Phase 7's design ADR (ha-8fyg).
Steps 2–5 are independent after step 1. Step 6 (ADR-023) gives Hirð code a
way to install the registry defaults spawned actors resolve tools through;
without it the demo cannot run from `hird run` alone. Step 7 requires all
of the others.

## Out of scope

- Erlang abstract forms backend (v0.2).
- Core Erlang backend (v0.3+).
- BEAM bytecode emission (never).
- Package management or dependency resolution.
- repl command is a stretch goal; OK to defer.

## Acceptance Criteria

- Erlang source emission from IR produces valid .erl files.
- Generated .erl files compile with stock `erlc` without errors.
- Runtime support library exists in runtime/ as hand-written Erlang:
  tool effect dispatcher, audit log sink, supervisor wiring, handler machinery.
- CLI binary `hird` with subcommands: check, build, run, emit-ast, emit-effect-graph.
- `hird check` type-checks source files and reports errors with source spans.
- `hird build` produces .erl files and compiles them with erlc.
- `hird emit-effect-graph` produces JSON showing actors, their effects, message
  types, and supervisor relationships.
- **The v0.1 demo runs end-to-end**: a supervised Planner actor compiles, runs on
  BEAM, produces audit log output, and the effect graph JSON is queryable.
- Test harness: mock handlers replace Tool<ReadRepo> and Tool<CreateTicket> for
  dry-run testing; test confirms planner produces expected ticket output from
  mock repo data.
- Integration tests: at least 5 small Hirð programs compile, run on BEAM, and
  produce expected output and audit logs.
- Generated Erlang is human-readable (inspectable, not obfuscated).
- `cargo clippy` and `cargo test` pass for `hird-codegen` and `hird-cli`.


## Notes

**2026-07-10T09:56:10Z**

Task 1 (hir-zp13, Erlang source emission from IR) is done: expression emitter, handler-map threading, tool dispatch call sites, and declaration span comments are in hird-codegen, erlc-validated. Steps 2-5 (hir-1dvq, hir-z9rn, hir-7oph, hir-y9jo) are now unblocked and independent.

**2026-07-10T12:28:13Z**

Task 2 (hir-1dvq, actor codegen to gen_server) is done: actors emit
gen_server behaviour modules per the locked mapping (per-constructor
call/cast dispatch, ReplyTo as From, explicit gen_server:reply,
registry-only handler resolution inside actors), erlc-validated.
Codegen's public API changed for multi-module output: emit_modules
returns (module, source) pairs — base module plus one per actor —
which hir-y9jo's `hird build` should consume. Remaining independent
steps: hir-z9rn (supervisor codegen), hir-7oph (runtime library),
hir-y9jo (CLI); hir-bxdd (demo) needs all of them.

**2026-07-10T13:37:43Z**

Task 3 (hir-z9rn, supervisor codegen) is done: supervisor declarations emit OTP supervisor behaviour modules ({local, Module} registration, verbatim strategy, child specs starting actor modules' start_link/1 with start_args rendered by the general expression emitter), erlc-validated and smoke-tested on BEAM (child starts, one_for_one restart works). emit_modules now returns base + actor + supervisor modules. Remaining independent steps: hir-7oph (runtime library), hir-y9jo (CLI); hir-bxdd (demo) needs both.

**2026-07-27T09:50:33Z**

Task 4 (hir-7oph, Erlang runtime support library) is done: runtime/ holds
hird_tool_dispatch, hird_audit, hird_handlers, hird_types, and a minimal
hird_sup_util (child_pid/2 only — generated supervisors inline their child
specs), each with -specs and eunit tests; the encoder reproduces the
conformance/v1 goldens byte-exactly. The dispatch contract gained an
injected caller id — hird_tool_dispatch:call(ToolName, Caller, Handlers,
Args) — and base modules now emit a hird_tools@/0 signature table for the
audit sink's type-directed encoding (ADR-022/016 amended). Verified end to
end on BEAM: supervised actor, registry mocks, audit lines, crash-restart.

Remaining: hir-y9jo (CLI; its `hird run` startup wiring should start
hird_audit and register each base module's hird_tools@/0), then hir-bxdd
(demo).

**2026-07-27T10:49:01Z**

Task 5 (hir-y9jo, CLI) is done: the hird binary implements check, build,
run, emit-ast, and emit-effect-graph. build writes generated + embedded
runtime .erl into _build/hird/ (overridable) and compiles with erlc; run
enters through a generated hird_boot module (starts hird_audit with the
stdout sink, registers each base module's hird_tools@/0, calls main with
an empty handler map) and requires fn main() → () with no residual
Tool<…> effects. The effect graph is a versioned serde projection in
hird-ir (effect_graph → EffectGraph, schema_version 1) shared with the
future MCP server; the CLI serializes or text-renders it. Verified on
BEAM: mocked tool calls audit to stdout; actors spawn/request/reply.

Only hir-bxdd (the v0.1 demo) remains; all of its dependencies are now
closed.

**2026-07-27T12:27:31Z**

Scope addition: hir-shiv (install blocks, ADR-023) inserted as step 6
before the demo. Rationale: handler maps never cross spawn (ADR-020 §6),
so actors resolve tools only through the runtime registry, and the only
installation API was Erlang — plain `hird run demo/planner.hird` would
crash-loop on {unhandled_tool, …}. ADR-023 locks a Hirð-level
`install { … } in e` form (arms checked like handle blocks, pure
handlers only, checker-known Install effect, dynamic-extent semantics
via hird_handlers:with_handlers), keeping the demo and its harness pure
Hirð. hir-bxdd now depends on hir-shiv.

**2026-07-27T14:05:17Z**

Task 6 (hir-shiv, install blocks) is done: the install keyword, handle-grammar
arms, checking (handle's structural + signature-directed checks, plus a new
pure-handler diagnostic C0051), the checker-known Install effect (registry
built-in, no user declaration needed), a dedicated IR node, and emission to
hird_handlers:with_handlers with the established keys and binary-fun entries.
Verified on BEAM end to end: a spawned actor's tool call resolves through the
handler main installs, the audit record appears on stdout, and the registry
is restored after the body — crash included, through the emitted path.
phrasebook.md documents the form next to Handle Blocks.

Only hir-bxdd (the v0.1 demo) remains; all of its dependencies are closed.

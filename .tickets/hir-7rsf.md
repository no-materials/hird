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

1. [ ] [hir-zp13](hir-zp13.md) — Erlang source emission from IR
2. [ ] [hir-1dvq](hir-1dvq.md) — Actor codegen to Erlang gen_server
3. [ ] [hir-z9rn](hir-z9rn.md) — Supervisor codegen to Erlang
4. [ ] [hir-7oph](hir-7oph.md) — Erlang runtime support library
5. [ ] [hir-y9jo](hir-y9jo.md) — CLI commands: check, build, run, emit
6. [ ] [hir-bxdd](hir-bxdd.md) — v0.1 demo: supervised agent planner end-to-end

Step 2 implements the mapping locked by Phase 7's design ADR (ha-8fyg).
Steps 2–5 are independent after step 1. Step 6 requires all of them.

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


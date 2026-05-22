---
id: hir-y9jo
status: open
deps: [hir-zp13]
links: []
created: 2026-05-22T21:41:40Z
type: task
priority: 1
assignee: nomaterials
parent: hir-7rsf
tags: [phase-9, cli]
---
# CLI commands: check, build, run, emit

Implement the hird CLI binary in hird-cli with subcommands for the v0.1
workflow.

**Subcommands**:

- `hird check <file|dir>` — type-check source files and report diagnostics.
  Runs the full pipeline through type inference and effect checking but does
  not emit Erlang. Exit code 0 on success, 1 on errors.

- `hird build <file|dir>` — compile to Erlang source and invoke erlc.
  Produces .erl files in a build output directory (default: _build/hird/).
  Then invokes erlc to produce .beam files. Reports compilation errors from
  both the Hirð compiler and erlc.

- `hird run <file|dir> [-- args]` — build then run on BEAM.
  Invokes erl with the compiled .beam files. Passes through arguments after --.
  Requires Erlang/OTP on PATH.

- `hird emit-ast <file> [--json]` — dump the typed AST.
  Default: human-readable pretty-print. --json: JSON serialization.

- `hird emit-effect-graph <file|dir> [--json]` — dump the actor/effect graph.
  Shows actors, their effects, message types, supervisor relationships, and
  tool effect declarations. Default: human-readable. --json: structured JSON
  suitable for MCP server consumption.

**CLI framework**: use clap for argument parsing.

**Error reporting**: diagnostics rendered to stderr with source spans. Use the
diagnostic infrastructure from Phase 2.

**Erlang detection**: check for erlc and erl on PATH. Produce a helpful error
if not found: "Erlang/OTP not found. Install from https://www.erlang.org/ or
use asdf/nix."

## Acceptance Criteria

- hird binary builds and runs.
- hird check type-checks and reports errors with source spans.
- hird build produces .erl and .beam files.
- hird run builds and launches on BEAM.
- hird emit-ast dumps typed AST (text and JSON).
- hird emit-effect-graph dumps actor/effect graph (text and JSON).
- Missing erlc/erl produces helpful error message.
- clap-based argument parsing with --help for all subcommands.
- At least one integration test per subcommand.


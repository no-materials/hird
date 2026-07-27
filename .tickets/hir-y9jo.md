---
id: hir-y9jo
status: closed
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


## Notes

**2026-07-27T10:13:36Z**

Design decisions for this ticket (agreed with nomaterials):

**`hird run` entry point — `fn main` convention, generated boot module.**
`hird run` requires a `fn main() → ()` in the compiled module and errors if
absent (no implicit "start all supervisors"; explicit over implicit). Arg
passthrough after `--` is reserved for a later `main(args)` arity. The CLI
enforces that main's residual effect row contains no unhandled `Tool<…>`
effects (per ADR-017's signature-directed checking), so an empty handler
map at the top level is a compile error rather than an ADR-022 runtime
dispatch crash. The caller id for main is the call-site literal
"Module.main" per ADR-022 as amended. Startup wiring is a small *generated
boot module* (starts hird_audit, registers each base module's
hird_tools@/0, calls main@ with an empty handler map) — not an `erl -eval`
string — so the startup sequence stays in inspectable .erl and the build
output runs on plain erl without the CLI.

**Effect graph — versioned projection type in hird-ir, not ad-hoc CLI JSON.**
Add `pub fn effect_graph(&IrModule) -> EffectGraph` (serde) to hird-ir; the
CLI only serializes it, and hir-q0as's MCP server consumes the same Rust
type — one schema, two frontends. Shape: top-level `schema_version` field;
types rendered both structurally and as canonical surface-syntax strings
(reuse the pretty-printer); source spans on every node. Schema evolves
additively only.

**Smaller calls.**
- `<file|dir>`: a directory compiles each .hird file as an independent
  module into one output dir (ADR-010 defers resolution); collide on
  generated module names → error.
- Runtime shipping: embed runtime/*.erl in the hird binary via
  include_str! and write them into the build dir; single self-contained
  binary.
- Audit sink for CLI runs defaults to stdout (ADR-016 wire format).

**2026-07-27T10:48:50Z**

Done. hird-ir gained the versioned EffectGraph projection (schema_version 1,
canonical + structural type rendering, per-node source lines); the CLI
implements check/build/run/emit-ast/emit-effect-graph per the design note:
miette-rendered diagnostics with spans, embedded runtime via include_str!,
generated hird_boot (audit sink on stdout, hird_tools@ registration, main
with empty handler map), fn-main validation (no params, unit return, no
residual Tool<…> effects), module-name collision detection, and erlc/erl
detection with install advice. 11 CLI integration tests cover every
subcommand plus the error paths; 3 hird-ir tests pin the graph shape.
Verified end to end on BEAM: a handle-mocked tool call runs and emits its
audit record on stdout, and a spawn/request/reply actor round-trips.
Arg passthrough after -- is reserved (rejected with a message) pending a
main(args) arity.

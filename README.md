# Hirð

A typed language for long-running agent systems on BEAM: effect-row
tracking, auditable tool effects, typed actors, and OTP supervision.
Python agent frameworks hide side effects in coroutine soup; Hirð makes
every tool call, every actor message, and every supervisor boundary
visible in the types and queryable by tooling.

## The v0.1 demo: a supervised agent planner

`demo/agent_planner.hird` is the flagship v0.1 program. A `Planner`
actor receives a repository path, reads repository state through
`Tool<ReadRepo>`, analyzes it (pure computation), files tickets through
`Tool<CreateTicket>`, and logs progress through `Tool<Log>`; a
`PlannerSup` supervisor declares it as a `one_for_one` child. The
entry point installs the demo's tool handlers in the runtime registry —
handler maps never cross `spawn`, so the `install` block is how the
spawned planner's tool calls resolve — then drives one planning round
end to end.

Build it (requires Erlang/OTP on `PATH`):

```sh
cargo run -p hird-cli -- build demo/agent_planner.hird
```

This type-checks the program, emits human-readable Erlang source
(`hird_agent_planner.erl`, `hird_planner.erl`, `hird_planner_sup.erl`,
plus the hand-written runtime), and compiles it all with `erlc` into
`_build/hird/`.

Run it on BEAM:

```sh
cargo run -p hird-cli -- run demo/agent_planner.hird
```

Every tool invocation — mocked or real — is recorded unconditionally on
the audit stream, one canonical JSON line per call:

```json
{"schema_version":1,"tool":"CreateTicket","args":{"body":"The parser has no fuzz harness.","title":"Fuzz the parser"},"result":{"ok":{"ctor":"TicketId","args":["Fuzz the parser"]}},"timestamp":"2026-07-28T06:44:42.893Z","caller":"AgentPlanner.file_tickets"}
```

Query the actor/effect graph as JSON (or drop `--json` for text):

```sh
cargo run -p hird-cli -- emit-effect-graph demo/agent_planner.hird --json
```

The graph shows the `Planner` actor with its full effect summary, its
mailbox sum type (`PlanRepo | GetStatus | Shutdown`), the `PlannerSup`
supervisor with its strategy and children, and each tool declaration
with structured argument and return types.

Other subcommands: `check` (type-check only), `emit-ast` (typed AST as
text or JSON).

The dry-run test harness lives in `crates/hird-cli/tests/demo.rs`: it
re-runs the same demo with mock handlers swapped into the `install`
block and asserts on the audit JSON lines — the same program, the same
unconditional audit stream, differing only in the installed handler
set.

## Repository layout

- `crates/` — the Rust compiler workspace (lexer, parser, checker, IR,
  codegen, CLI).
- `runtime/` — the hand-written Erlang runtime support library (tool
  dispatch, audit sink, handler registry).
- `demo/` — the v0.1 demo program.
- `docs/` — normative specifications (grammar, error model, tool
  effects wire format).
- `phrasebook.md` — dense surface-syntax reference.
- `DECISIONS.md` — architecture decision records.

## Development

MSRV is Rust 1.92 (edition 2024). Before sending changes:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

BEAM-dependent tests skip themselves when `erlc` is not on `PATH`.

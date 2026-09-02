# Hirð

![The king's hirð wading a river, drawn by Erik Werenskiold](img/hero.jpg)

*A hirð is a Norse king's household guard: sworn retainers, each with a
named duty, answerable to one lord. Illustration by Erik Werenskiold for
Magnús Erlingsson's saga in Snorri's Heimskringla (public domain, via
[Wikimedia Commons](https://commons.wikimedia.org/wiki/File:Magnus_Erlingssons_saga-Over_aaa-Werenskiold.jpg)).*

A typed language for long-running agent systems on BEAM: effect-row
tracking, auditable tool effects, typed actors, and OTP supervision.
Python agent frameworks hide side effects in coroutine soup; Hirð makes
every tool call, every actor message, and every supervisor boundary
visible in the types and queryable by tooling.

**What that buys: deterministic replay of real agent traffic.** Every
tool call is recorded unconditionally in a canonical wire format, so a
recorded run is a file you can replay — the same calls, in the same
order, each served the result the recorded run got back, with no service
contacted. That is a regression test with no oracle to maintain, a bug
report that reproduces, and a fixed environment to evaluate a change in.
It is also not something you can retrofit onto a framework that hides
its side effects: it needs the effects in the types and a single
dispatch path underneath them. `hird demo` is that claim in one command:
it records a run of the demo planner, replays that one recording against
three variants of the program, and prints where each parted from it.

**And systems that stand.** A Hirð program is not a script that exits:
`fn main` can start a supervision tree and `stand`, leaving typed actors
serving after its own work is done — driving their own periodic rounds
off a clock capability, crashing and restarting under a declared budget,
every round on the audit stream. `hird run demo/agent_fleet` is that
claim running: a hirð of three retainers that keeps working through a
deliberate crash.

**Status**: pre-1.0 and experimental. The v0.1 compiler pipeline works
end to end (the demos below type-check, compile to Erlang, and run on
BEAM), but the language surface is unstable, nothing is published to
crates.io, and breaking changes land without deprecation cycles. The
roadmap lives in the in-repo issue tracker (see `.beads/README.md`).

## Install

Prebuilt binaries for Linux, macOS, and Windows are attached to every
[release](https://github.com/no-materials/hird/releases): extract the
archive for your platform and put `hird` (the compiler), `hird-lsp`, and
`hird-mcp` on your `PATH`.

From source, with Rust 1.97 or newer:

```sh
cargo install --git https://github.com/no-materials/hird hird-cli
cargo install --git https://github.com/no-materials/hird hird-lsp  # optional
cargo install --git https://github.com/no-materials/hird hird-mcp  # optional
```

With Nix, the same three binaries are flake outputs
(`nix run github:no-materials/hird#hird-mcp`).

Compiling and running programs needs **Erlang/OTP** on `PATH`
(`apt install erlang`, `brew install erlang`, …); `hird check` works
without it.

## Quick start

Hirð has no ambient `print`. Anything a program tells the outside world
goes through a *tool* — a declared, typed, audited external operation —
so the smallest observable program is a tool call. Save this as
`hello.hird`:

```
module Hello

tool Say : { message: String } → ()

fn quiet_say(args: { message: String }) → () = ()

fn main() → () ! {} =
  handle {
    Tool<Say> → quiet_say,
  } in say({ message: "hello, world" })
```

```sh
hird run hello.hird
```

```json
{"schema_version":1,"tool":"Say","args":{"message":"hello, world"},"result":{"ok":null},"timestamp":"…","caller":"Hello.main"}
```

Three things happened. Declaring `tool Say` created the effect
`Tool<Say>` and a callable `say`. The `handle` block supplied an
implementation and discharged that effect, so `main` is honestly `! {}`.
And the call was recorded on the audit stream — unconditionally, because
mocked and real tool calls audit identically. ASCII operator spellings
(`->`) normalise to their Unicode forms (`→`) at lex time, so either is
legal input.

| Command | What it does |
|---|---|
| `hird check <file-or-dir>` | type- and effect-check; coded diagnostics |
| `hird build <file-or-dir>` | emit readable Erlang, compile it to `.beam` |
| `hird run <file-or-dir>` | build, then execute `fn main` on BEAM |
| `hird demo` | record one run of the built-in demo, replay it against variants of the program |
| `hird emit-ast <file> --json` | the typed IR of every definition |
| `hird emit-effect-graph <file-or-dir> --json` | actors, mailboxes, handler rows, supervisors, tools |

[`docs/writing-hird-human.md`](docs/writing-hird-human.md) is the guided
tour, and [`phrasebook.md`](phrasebook.md) the dense syntax reference.

## The flagship demo: a standing hirð of agents

A hirð is retainers with named duties; `demo/agent_fleet/` is the
metaphor made literal. Three supervised actors serve for as long as the
program stands: a `Planner` ticks itself on a clock and forges each
round's order (pure planning imported from a second module — the source
spans a real `use` boundary), an `Executor` carries the order out
through `Tool<RunErrand>` and reports onward, an `Auditor` chronicles
every outcome through `Tool<Chronicle>`. Round 3 crashes the executor
*on purpose*: `FleetSup` restarts `rest_for_one`, so the auditor —
downstream of the crash — restarts with it, the planner keeps its round
counter, and the rounds keep coming. Actor state dies with its process;
the audit stream is the durable record.

```sh
hird run demo/agent_fleet
```

```json
{"schema_version":1,"tool":"RunErrand","args":{"errand":"mend the palisade","round":2},"result":{"ok":"done"},"timestamp":"…","caller":"Executor.handle_msg/Carry"}
{"schema_version":1,"tool":"Chronicle","args":{"note":"done","round":2},"result":{"ok":null},"timestamp":"…","caller":"Auditor.handle_msg/Record"}
{"schema_version":1,"tool":"Log","args":{"level":"info","message":"executor takes its post"},"result":{"ok":null},"timestamp":"…","caller":"Executor.init"}
{"schema_version":1,"tool":"Log","args":{"level":"info","message":"auditor takes its post"},"result":{"ok":null},"timestamp":"…","caller":"Auditor.init"}
{"schema_version":1,"tool":"RunErrand","args":{"errand":"scout the border","round":4},"result":{"ok":"done"},"timestamp":"…","caller":"Executor.handle_msg/Carry"}
```

Round 3 never beats — the crash consumed its order — and the two
re-posted inits are the supervisor's work, visible in the same stream as
everything else. The tree itself is queryable; its effect graph is the
system's live org chart, every retainer with its duty and its effects:

```sh
hird emit-effect-graph demo/agent_fleet
```

## Record and replay a run

`demo/agent_planner.hird` drives one planning round against a supervised
`Planner`: repository state in through `Tool<ReadRepo>`, pure analysis,
tickets out through `Tool<CreateTicket>`, progress through `Tool<Log>`.
Every tool invocation — mocked or real — lands on the audit stream, one
canonical JSON line per call:

```json
{"schema_version":1,"tool":"CreateTicket","args":{"body":"The parser has no fuzz harness.","title":"Fuzz the parser"},"result":{"ok":{"ctor":"TicketId","args":["Fuzz the parser"]}},"timestamp":"2026-07-28T06:44:42.893Z","caller":"AgentPlanner.file_tickets"}
```

Because the stream is complete — every call, full arguments, tagged
result — a recorded run is a replayable environment:

```sh
hird run demo/agent_planner.hird --audit-file run.jsonl   # record
hird run demo/agent_planner.hird --replay run.jsonl       # replay
```

The replay cursor outranks every `handle` and `install` block, so no
tool runs and no service is contacted; each call receives its logged
result, failures included. Matching is strict: the call at each position
must be the one the log recorded there, or the run crashes with a
`replay_divergence` naming the position, the recorded call and the
offered one — and a log the run did not read to the end fails too.

So a checked-in recording is a regression test with no oracle to
maintain: `demo/agent_planner.golden.jsonl` is one run of the planner,
replayed by the demo suite in CI, and the build fails the moment the
program's decisions drift from it. And because the log serves every
result, one recording is a fixed environment to compare variants of a
program in — every arm meets a byte-identical world, so what differs is
attributable to the programs:

```
baseline        agreed with all 7 calls
announce-first  parted at call 2 (tool_mismatch)
eager           parted at call 4 (args_mismatch)
```

That evaluation is `hird demo`: no arguments, nothing to install beyond
Erlang, and nothing checked in that it has to be trusted about — it
writes the planner and the two edited variants into `_build/hird-demo`,
records the episode itself, and replays it against all three.

[`docs/audit-evidence.md`](docs/audit-evidence.md) states what the
stream guarantees and what it does not;
[`docs/tool-effects.md`](docs/tool-effects.md) is the normative format
and replay specification.

## LLM tooling (MCP)

[![hird-mcp on Glama](https://glama.ai/mcp/servers/no-materials/hird/badges/score.svg)](https://glama.ai/mcp/servers/no-materials/hird)

`hird-mcp` is a Model Context Protocol server over the same compiler
pipeline, speaking stdio. It gives LLM agents structured compiler
queries instead of source-reading guesswork: `infer_type`,
`lookup_definition`, `explain_effect_row`, `render_ir_fragment`,
`explain_actor_protocol`, `emit_actor_effect_graph`,
`get_context_for_symbol` (token-budget-aware symbol summaries), and
`get_context_budget`. Errors come back structured — undefined names
list the available ones, parse and type errors carry coded
diagnostics — so agents can self-correct from tool output alone.

The repository ships a project-scoped `.mcp.json`, so Claude Code
sessions started here pick the server up automatically (it launches
`nix run .#hird-mcp`; run `nix build .#hird-mcp` once so the first
session start doesn't wait on a cold build). Any other MCP client can
launch the `hird-mcp` binary directly, with no arguments.

Things worth asking an agent wired to it:

- "What does the Planner actor in demo/agent_planner.hird do? Ask the
  compiler instead of reading the source."
- "If the Executor in demo/agent_fleet crashes mid-round, who restarts
  it, who restarts with it, and what's the restart budget?"
- "Give me a 50-token summary of the Planner actor. Now 400 tokens.
  What got dropped?"
- "Write a new Hirð module with a supervised actor, and iterate with
  the hird tools until they confirm it's clean."

`demo/counter_demo.hird` is that last prompt's output: a supervised
counter written by an LLM agent that verified itself against the MCP
tools alone — it type-checks and runs on BEAM unmodified. And
`demo/heartbeat.hird` is the smallest standing program: one actor, one
clock, one beat a second until Ctrl-C. `docs/writing-hird-llm.md` is
the agent-facing guide.

## Editor support

`hird-lsp` is a Language Server Protocol server over the compiler front
end, speaking stdio: diagnostics on open and save, hover with inferred
types and effect rows, go-to-definition for top-level declarations.
Point any LSP client at the binary, with no arguments.
`tree-sitter-hird/` is a tree-sitter grammar for the v0.1 surface, with
highlight, indent, and fold queries, built by the flake as a package
output. [`docs/editor-setup.md`](docs/editor-setup.md) has client
configuration (including Neovim, with and without nix), the grammar
development loop, and the v0.1 limitations.

## Repository layout

- `crates/` — the Rust compiler workspace (lexer, parser, checker, IR,
  codegen, CLI, LSP and MCP servers).
- `tree-sitter-hird/` — the tree-sitter grammar and editor queries.
- `runtime/` — the hand-written Erlang runtime support library (tool
  dispatch, audit sink, handler registry).
- `demo/` — the v0.1 demo programs.
- `conformance/` — golden files for the audit-log wire format.
- `docs/` — normative specifications (grammar, error model, tool
  effects wire format), the audit stream's guarantees, and editor setup.
- `phrasebook.md` — dense surface-syntax reference.
- `DECISIONS.md` — architecture decision records.
- `.beads/README.md` — the issue tracker and roadmap, driven by `bd`.

## Development

MSRV is Rust 1.97 (edition 2024). Before sending changes:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

BEAM-dependent tests skip themselves when `erlc` is not on `PATH`.

[`CONTRIBUTING.md`](CONTRIBUTING.md) has the rest: the dev shell, the
checks CI runs beyond those three, what "done" means, and how to report
a bug or file an issue from outside the repository.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.

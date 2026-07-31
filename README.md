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

**Status**: pre-1.0 and experimental. The v0.1 compiler pipeline works
end to end (the demos below type-check, compile to Erlang, and run on
BEAM), but the language surface is unstable, nothing is published to
crates.io, and breaking changes land without deprecation cycles. The
roadmap lives in `.tickets/`.

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

effect Tool<t>

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
| `hird build <file>` | emit readable Erlang, compile it to `.beam` |
| `hird run <file>` | build, then execute `fn main` on BEAM |
| `hird emit-ast <file> --json` | the typed IR of every definition |
| `hird emit-effect-graph <file> --json` | actors, mailboxes, handler rows, supervisors, tools |

[`docs/writing-hird-human.md`](docs/writing-hird-human.md) is the guided
tour, and [`phrasebook.md`](phrasebook.md) the dense syntax reference.

## The v0.1 demo: a supervised agent planner

`demo/agent_planner.hird` is the flagship v0.1 program. A `Planner`
actor receives a repository path, reads repository state through
`Tool<ReadRepo>`, analyzes it (pure computation), files tickets through
`Tool<CreateTicket>`, and logs progress through `Tool<Log>`; a
`PlannerSup` supervisor declares it as a `one_for_one` child. The
entry point installs the demo's tool handlers in the runtime registry —
handler maps never cross the supervision boundary, so the `install`
block is how the supervised planner's tool calls resolve — starts the
tree with `supervise(PlannerSup)`, reaches the running child with
`child(PlannerSup, planner)`, and drives one planning round end to end:
the planner it messages is a supervised OTP process, restarted by
`PlannerSup` if it crashes.

Build it (requires Erlang/OTP on `PATH`):

```sh
hird build demo/agent_planner.hird
```

This type-checks the program, emits human-readable Erlang source
(`hird_agent_planner.erl`, `hird_planner.erl`, `hird_planner_sup.erl`,
plus the hand-written runtime), and compiles it all with `erlc` into
`_build/hird/`.

Run it on BEAM:

```sh
hird run demo/agent_planner.hird
```

Every tool invocation — mocked or real — is recorded unconditionally on
the audit stream, one canonical JSON line per call:

```json
{"schema_version":1,"tool":"CreateTicket","args":{"body":"The parser has no fuzz harness.","title":"Fuzz the parser"},"result":{"ok":{"ctor":"TicketId","args":["Fuzz the parser"]}},"timestamp":"2026-07-28T06:44:42.893Z","caller":"AgentPlanner.file_tickets"}
```

Query the actor/effect graph as JSON (or drop `--json` for text):

```sh
hird emit-effect-graph demo/agent_planner.hird --json
```

The graph shows the `Planner` actor with its full effect summary, its
mailbox sum type (`PlanRepo | GetStatus | Shutdown`), the `PlannerSup`
supervisor with its strategy and children, and each tool declaration
with structured argument and return types.

The dry-run test harness lives in `crates/hird-cli/tests/demo.rs`: it
re-runs the same demo with mock handlers swapped into the `install`
block and asserts on the audit JSON lines — the same program, the same
unconditional audit stream, differing only in the installed handler
set.

## LLM tooling (MCP)

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
- "If the Planner crashes mid-plan, who restarts it, and what's the
  restart budget?"
- "Give me a 50-token summary of the Planner actor. Now 400 tokens.
  What got dropped?"
- "Write a new Hirð module with a supervised actor, and iterate with
  the hird tools until they confirm it's clean."

`demo/counter_demo.hird` is that last prompt's output: a supervised
counter written by an LLM agent that verified itself against the MCP
tools alone — it type-checks and runs on BEAM unmodified.
`docs/writing-hird-llm.md` is the agent-facing guide.

## Editor support

### Language server

`hird-lsp` is a Language Server Protocol server over the compiler front
end, speaking stdio. Point any LSP client at the `hird-lsp` binary, with
no arguments.

v0.1 capabilities:

- **Diagnostics** on file open and save: parse errors, then type errors
  and warnings, with source spans.
- **Hover**: the inferred type of the identifier or expression under the
  cursor, including the effect row for functions
  (`read_file : Path → String ! {Tool<ReadFile>}`).
- **Go-to-definition** for top-level declarations: functions, types and
  their constructors, effects, tools (by marker or generated function
  name), actors and their message types, and supervisors.

Known limitations (real, by design for v0.1):

- No completion, rename/refactor, or code actions.
- No workspace-wide analysis: each file is compiled alone, so `use`
  imports of other modules report as unresolved and definitions resolve
  only within the current file.
- No incremental compilation: every change recompiles the whole file.

### Syntax highlighting

`tree-sitter-hird/` is a tree-sitter grammar for the v0.1 surface, with
`highlights.scm`, `indents.scm` and `folds.scm` under `queries/`. Both
operator spellings parse identically, so `→` and `->` highlight the same.
The flake builds it as a package output, next to `hird-lsp`:

```sh
nix build github:no-materials/hird#tree-sitter-hird
```

The result holds the compiled `parser` and a copy of `queries/`. A
flake-based Neovim configuration takes this repository as an input and
hands the grammar to nvim-treesitter, which wants the parser and the
queries under the names it looks them up by:

```nix
# inputs.hird.url = "github:no-materials/hird";
{
  plugins = [
    (pkgs.neovimUtils.grammarToPlugin
      inputs.hird.packages.${pkgs.system}.tree-sitter-hird)
  ];
}
```

Neovim needs the file type registered too, whichever route below you
take, since `.hird` is not one it knows:

```lua
vim.filetype.add({ extension = { hird = "hird" } })
```

Without nix, nvim-treesitter builds the grammar itself, given the
tree-sitter CLI on `PATH` (`npm i -g tree-sitter-cli`). `src/parser.c`
is generated rather than committed — `grammar.js` is the only source —
so `requires_generate_from_grammar` is the part that matters: it makes
`:TSInstall hird` generate the parser before compiling it.

```lua
require('nvim-treesitter.parsers').get_parser_configs().hird = {
  install_info = {
    url = "https://github.com/no-materials/hird",
    location = "tree-sitter-hird",
    files = { "src/parser.c", "src/scanner.c" },
    requires_generate_from_grammar = true,
  },
  filetype = "hird",
}
```

That installs the parser but not the queries: nvim-treesitter ships
those only for the languages it supports, so copy this grammar's onto
the runtime path by hand. It is the one step the nix package does for
you.

```sh
mkdir -p ~/.config/nvim/queries/hird
cp tree-sitter-hird/queries/*.scm ~/.config/nvim/queries/hird/
```

With no plugin at all, build the parser straight onto the runtime path
next to those queries (`tree-sitter build -o ~/.config/nvim/parser/hird.so`)
and call `vim.treesitter.start()` from a `FileType hird` autocommand.

Working on the grammar itself needs no global tree-sitter CLI — the dev
shell ships one, and `nix flake check` runs the corpus tests and parses
every `.hird` source in the repository:

```sh
cd tree-sitter-hird && tree-sitter generate && tree-sitter test
```

## Repository layout

- `crates/` — the Rust compiler workspace (lexer, parser, checker, IR,
  codegen, CLI, LSP and MCP servers).
- `tree-sitter-hird/` — the tree-sitter grammar and editor queries.
- `runtime/` — the hand-written Erlang runtime support library (tool
  dispatch, audit sink, handler registry).
- `demo/` — the v0.1 demo programs.
- `conformance/` — golden files for the audit-log wire format.
- `docs/` — normative specifications (grammar, error model, tool
  effects wire format).
- `phrasebook.md` — dense surface-syntax reference.
- `DECISIONS.md` — architecture decision records.
- `.tickets/` — the issue tracker and roadmap, as plain markdown.

## Development

MSRV is Rust 1.97 (edition 2024). Before sending changes:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

BEAM-dependent tests skip themselves when `erlc` is not on `PATH`.

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

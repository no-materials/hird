# Changelog

All notable changes to Hirð are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/), with the pre-1.0 caveat that
minor versions may break the language surface and the tooling APIs.

The tree-sitter grammar (`tree-sitter-hird/`) versions on its own
schedule and is not covered by these entries.

## [Unreleased]

### Changed

- **MCP tool descriptors disclose more.** Every `hird-mcp` tool now carries
  MCP annotations (`readOnlyHint`, `idempotentHint`, no destructive or
  open-world behaviour), a `title`, and an `outputSchema` matching its
  `structuredContent`. Descriptions state the result fields, the error codes
  a call can fail with, and which sibling tool to prefer for related
  questions; parameter descriptions spell out path resolution, character
  columns, actor-name scope, and how `budget` drops sections.

## [0.3.0] — 2026-09-03

### Added

- **Record update.** A record literal may end in `..base`:
  `{ beats: st.beats + 1, ..st }` takes the listed fields from the literal
  and every other field from `st`, and has `st`'s type, so an update can
  neither add (C0010) nor remove fields. `{ ..base }` alone is an error
  (C0060), not a copy; the base is written once, last (P0008). Lowers to
  an Erlang map update. The demos' actor states are now record aliases
  read by field and rebuilt with `..st`.
- **Type aliases.** `type alias Name<params> = T` names a type expression
  — a record, a tuple, a function type — and every use expands to it, so
  an alias has no identity: it is transparent to unification, the wire
  check, the audit format, the IR, and codegen. A tool's argument record
  and the handlers that implement it now name the shape once (the demos
  do: `LogArgs` and friends). A recursive alias is C0059 and `opaque` on
  an alias is P0007; `pub type alias` exports it and an importer sees the
  expansion.
- **Sequencing with `;`.** `a; b` runs `a` for its effects and evaluates
  to `b`; it is sugar for `let _ = a in b`, except that `a` must be `()`
  (C0058 otherwise), so a result is never dropped by accident. Right-
  associative and looser than every operator; bodies remain single
  expressions. The demos and guides use it for effect sequencing.
- **`Option` is predeclared.** `Option<a> = Some(a) | None` is seeded like
  `Next`, so `Some(1)` and a `match` over `Some`/`None` need no declaration;
  a user `type Option<a> = …` still shadows it. `List` stays a bare type
  name until list literals and patterns exist.
- **Irrefutable patterns in `let`.** The binder is a pattern:
  `let Config(clock, period) = config in …`, `let (a, b) = pair in …`, and
  `let _ = e in …` all work, replacing the one-arm `match` idiom; a `_`
  binder emits `_ = Expr` in Erlang, so sequencing an effect no longer
  needs an invented name. A plain
  name still generalises; a destructuring pattern binds monomorphically and
  must cover every value of the bound type — a refutable one (`Some(x)`, a
  literal) is a compile error (C0057). Lowers to a one-arm match, so the IR
  and codegen are unchanged.

### Changed

- **Actor handlers and `init` no longer carry return types.** A handler's
  outcome is always `Next<State>` and `init` always returns the state, both
  fixed by the actor's `state:` field, so the grammar drops the slot:
  `handle Tick, st ! {…} = …` and `init: fn(c: Cfg) ! {…} = …`. Writing a
  `→ …` there is now a parse error (P0006) with a removal hint. The IR
  pretty-printer prints the new form; demos, fixtures, and docs follow.
- **Built-in effect heads need no declaration.** `Tool`, `Send`, `Await`,
  `Spawn`, `Schedule`, and `Exn` are pre-declared like `Install`,
  `Supervise`, `Stand`, and `Clock`, so a module no longer opens with
  `effect Tool<t>` before its rows may name a tool call or a message send.
  A declaration of a built-in head is accepted with a warning (C0056) and
  otherwise ignored; user heads (`effect Audit<t>`) are declared as before.
  The IR pretty-printer no longer synthesises declarations for built-in
  heads. The demos, fixtures, and docs drop the redundant lines.

## [0.2.0] — 2026-09-01

### Added

- **Time as a capability.** `Clock` is a built-in opaque type; `clock()`
  acquires the runtime clock with the checker-known bare effect `Clock`,
  and `schedule(clock, pid, msg, delay_ms)` delivers a message to a typed
  reference after a delay (an `Int` of milliseconds) with the effect
  `Schedule<Msg>` — a head of its own, so the row tells a self-driving
  actor from one that merely sends. `self()` is the enclosing actor's own
  `Pid<Msg>` (C0055 outside actor bodies), so an actor can schedule its
  own next tick. A supervisor child's `start_args` may acquire the clock
  — the one effect it is allowed — and the supervisor's derived row
  records it. Scheduled messages cannot be cancelled. `clock` is
  contextual (only `clock()` is the form); `schedule` and `self` are
  reserved. Lowers through the new `hird_clock` runtime module.
  `demo/heartbeat.hird` is a standing actor that beats once a second.
- **Request timeouts.** `request(pid, Ctor, timeout_ms)` overrides the
  5000 ms default with an `Int` of milliseconds; the row is unchanged, a
  timeout still exits the caller.
- **Standing programs.** The `stand()` keyword form keeps a program up
  after `main`'s setup: it blocks until the program is asked to stop, then
  shuts down every supervisor the caller started (OTP parent shutdown,
  reverse start order) and returns, so the audit stream is synced before
  the halt. It carries the checker-known bare effect `Stand`. Two stop
  triggers exist: SIGTERM, where the platform has it, and end of file on
  the emulator's stdin when the launcher owns that pipe — which `hird run`
  does on every platform, closing it on Ctrl-C or termination, so a
  standing program ends cleanly from the terminal on Windows as well as
  Unix. Without `stand()` a program halts when `main` returns, as before.
- **Actor stop path.** A message handler returns the built-in
  `Next<State>` sum — seeded as if `type Next<a> = Continue(a) | Stop`
  were declared — instead of a bare state, so an actor can stop itself
  deliberately. `Stop` exits with reason `normal`: a `transient` child
  stays stopped, a `permanent` one is restarted, by stock OTP semantics.
  `init` still returns the bare state. Every handler body changes:
  wrap a continuing state in `Continue(…)`.
- **Group restart strategies.** `one_for_all` and `rest_for_one` now
  lower to their OTP supervisor strategies instead of warning as
  unimplemented (C0050 is retired): a crash restarts the whole group, or
  the crashed child and every child after it, under the declared
  `intensity`/`period` budget.
- **The standing-fleet demo.** `demo/agent_fleet/` is the flagship
  standing program: three supervised actors with typed protocols —
  a clock-driven planner, an executor, an auditor — under one
  `rest_for_one` tree, with a deliberate crash whose restart is visible
  on the audit stream. Its source spans two modules joined by `use`,
  compiled as one program by `hird build <dir>`.

### Fixed

- **Unqualified imported functions miscompiled to the importing
  module.** A selective import (`use Lib.{f}`) used bare compiled to a
  call on the importing module and crashed at runtime with
  `function not exported`; such uses now resolve to the defining module,
  in call and value position alike, with local shadows respected.
  Qualified calls (`Lib.f(…)`) were always correct.
- **erlc unused-variable warnings on emitted code.** Effect-only
  bindings (`let logged = log(…) in …`) emitted binders erlc flagged as
  unused — dozens of stderr lines drowning the audit stream on `hird
  run`. Unreferenced binders now emit `_`-prefixed, and the erlc
  validation suite rejects the warning class.

### Changed

- **MCP callees resolve to their defining module.**
  `get_context_for_symbol` reports an imported callee as `Lib.f` whether
  the body writes it qualified or bare.

## [0.1.1] — 2026-08-27

### Added

- **Audit-log record and replay.** `hird build` and `hird run` gain
  `--audit-file <path>` to route the audit stream to an append-only file
  (stdout stays the default) and `--replay <log.jsonl>` to run a program
  against a recorded log: the runtime's replay cursor answers every tool
  call from the log, matches strict-sequentially, crashes with a
  structured `replay_divergence` naming the position and mismatch, and
  requires the log fully consumed by the end of the run.
- **Erlang-side log decoding.** The runtime decodes audit logs as a strict
  mirror of the Rust reference decoder; the conformance goldens round-trip
  byte-exactly through decode and re-encode.
- **Err-tagged audit records.** A handler that throws a domain failure is
  recorded with an `err` result and rethrown unchanged; any other
  exception class is a crash, propagated untouched and unrecorded.
- **Replay as a regression golden.** `demo/agent_planner.golden.jsonl` is
  one recorded run of the planner demo; the demo test suite replays it
  and asserts a drifted variant diverges at a named position.
- **`hird demo`.** Writes the embedded planner and two edited variants,
  records one run of the control arm, replays that recording against all
  three, and reports where each arm parts from it.
- **Tree-sitter grammar** with highlights, indents and folds queries,
  exposed as a flake package alongside `hird-lsp`, plus documentation of
  the non-nix route to it.
- **Docs.** `docs/audit-evidence.md` states what the audit stream
  guarantees, what it deliberately does not, and when the wire format may
  change; `CONTRIBUTING.md` describes the dev shell, what CI enforces, and
  how outside contributions reach the in-repo tracker.
- **CI** now installs OTP and runs the Erlang runtime suites and every
  BEAM-dependent test, with `HIRD_REQUIRE_BEAM` turning their self-skip
  into a failure.

### Changed

- **Cross-module analysis in the LSP and MCP servers.** Both servers used
  to compile each file as a single-module program, so `use` imports never
  resolved. They now compile the queried or open file's directory as one
  program, naming modules from file stems as the CLI does.
  - `hird-lsp`: diagnostics no longer flag resolvable imports; hover and
    go-to-definition resolve names imported selectively or as
    `Qualifier.member`, with locations in the defining file whether or not
    it is open; open buffers stand in for their files on disk; a save
    republishes every open document of the directory.
  - `hird-mcp`: the cache is one entry per directory, invalidated when any
    member's text changes. `lookup_definition`, `infer_type`,
    `render_ir_fragment`, `explain_effect_row` and `get_context_for_symbol`
    resolve symbol arguments as the file's source would and name the
    defining `file` in their responses; callers in
    `get_context_for_symbol` are found program-wide. A sibling that fails
    to parse is left out of the program (and listed under
    `siblings_with_parse_errors` when the importer then fails); one with
    type errors stays checked but has no IR.
- Package versions derive from one manifest each: crates inherit the
  workspace version, and the flake reads the numbers it stamps from
  `Cargo.toml` and `tree-sitter.json`.
- Issue tracking moved from `.tickets/` to Beads (`bd`), preserving ids.

### Fixed

- `hird-mcp` returned no `doc` for `pub` declarations: the leading comment
  sits inside the visibility node, which doc extraction never entered.

## [0.1.0] — 2026-07-30

The v0.1 milestone: a typed language for agent systems on BEAM with
effect-row tracking, auditable tool effects, typed actors, and OTP
supervision, plus LSP and MCP servers over the same compiler pipeline.

[0.3.0]: https://github.com/no-materials/hird/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/no-materials/hird/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/no-materials/hird/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/no-materials/hird/releases/tag/v0.1.0

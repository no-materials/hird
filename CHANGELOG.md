# Changelog

All notable changes to Hirð are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/), with the pre-1.0 caveat that
minor versions may break the language surface and the tooling APIs.

The tree-sitter grammar (`tree-sitter-hird/`) versions on its own
schedule and is not covered by these entries.

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

[0.1.1]: https://github.com/no-materials/hird/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/no-materials/hird/releases/tag/v0.1.0

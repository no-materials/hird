---
id: hir-8unj
status: closed
deps: []
links: []
created: 2026-05-22T21:35:41Z
type: task
priority: 1
assignee: nomaterials
parent: hir-b9gf
tags: [phase-0, workspace]
---
# Scaffold compiler crate topology

Split the current `crates/hird` into the compiler's crate structure. Each crate
is a workspace member under `crates/`, follows `#![no_std]` by default (except
`hird-cli` which requires std), and inherits workspace lints.

Crates to create:
- `hird-lex` — lexer, token definitions, span types
- `hird-parse` — parser, CST construction
- `hird-ast` — typed AST data structures
- `hird-types` — type representation, unification, inference engine
- `hird-effects` — effect row types, effect inference, handler lowering
- `hird-actors` — actor type system, typed Pid, message protocol types
- `hird-ir` — IR data structures, lowering, serialization
- `hird-codegen` — Erlang source emission from IR
- `hird-cli` — CLI binary (uses std), subcommand dispatch
- `hird-mcp` — MCP server for compiler introspection
- `hird-lsp` — LSP server scaffold

Dependency direction: lex <- parse <- ast <- types <- effects <- actors.
ir depends on ast + types + effects + actors. codegen depends on ir. cli
depends on everything. mcp and lsp depend on ir + types.

The original `crates/hird` becomes a facade crate re-exporting public API.
Or, if cleaner, remove it and have `hird-cli` as the user-facing entry point.

Create `runtime/` directory with a README explaining its purpose (Erlang
support library, not a Rust crate).

Update `examples/` and `wind_tunnel/` Cargo.toml to depend on appropriate
sub-crates instead of monolithic `hird`.

## Acceptance Criteria

- All 11 crates exist as workspace members.
- Each crate has `#![no_std]` (except hird-cli) and workspace lints.
- Workspace Cargo.toml lists all members.
- Internal dependency graph is declared in Cargo.toml files.
- `cargo fmt --all` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` pass.
- `runtime/` directory exists with README.md.
- CI remains green.


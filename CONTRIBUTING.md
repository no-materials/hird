# Contributing to Hirð

Thanks for looking. Hirð is pre-1.0 and experimental: the compiler
pipeline works end to end, but the language surface is unstable and
breaking changes land without deprecation cycles. Contributions are
welcome on that understanding.

Small, self-contained changes — a bug fix with a test, a diagnostic that
reads better, a doc correction, an editor-integration fix — can go
straight to a pull request. For anything larger, open an issue or a
[discussion](https://github.com/no-materials/hird/discussions) first:
the architecture is deliberately constrained, and a design that
conflicts with those constraints is cheaper to redirect before it is
written than after.

Three files define the constraints, and are worth reading before a
non-trivial change:

- [`AGENTS.md`](AGENTS.md) — the working agreement for this repository
  (written for AI coding agents, but it is the same agreement for
  humans): the north star, the definition of done, and the workspace
  policies.
- [`DECISIONS.md`](DECISIONS.md) — architecture decision records.
  Accepted decisions are immutable; changing one means a new record that
  supersedes it, not an edit.
- [`phrasebook.md`](phrasebook.md) — the canonical surface syntax. Any
  change to what Hirð source looks like is a change to this document
  too.

## Development environment

With Nix, the dev shell pins the whole toolchain — Rust 1.97.1 with the
components and targets CI uses, Erlang/OTP, the tree-sitter CLI and
Node, and the `bd` issue tracker:

```sh
nix develop
```

With direnv, put `use flake .` in an `.envrc` (it is gitignored, so
create your own) and `direnv allow`.

Without Nix, `rust-toolchain.toml` pins the same Rust version, so
rustup picks it up on first `cargo` invocation. Install Erlang/OTP
separately (`apt install erlang`, `brew install erlang`, …) if you want
the BEAM-dependent tests to run rather than skip.

MSRV is Rust 1.97 (edition 2024), restated in several places:
`rust-version` in `Cargo.toml`, `channel` in `rust-toolchain.toml`,
`RUST_MIN_VER` and `RUST_STABLE_VER` in `.github/workflows/ci.yml`,
`README.md`, and `AGENTS.md`. A bump has to move all of them.

## Build and test

The three commands that matter most:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

CI checks more than that. Reproducing the rest locally, in rough order
of how often it catches something:

```sh
taplo fmt --check --diff                       # TOML formatting
bash .github/copyright.sh                      # license headers on .rs files
./runtime/test.sh                              # Erlang runtime eunit suites
cargo doc --workspace --all-features --no-deps --document-private-items
nix flake check                                # includes the tree-sitter grammar
typos                                          # spelling, configured in .typos.toml
```

`taplo` and `typos` are the two of those the dev shell does not ship;
install them (`cargo install taplo-cli typos-cli`) or leave them to CI.

Some notes on the parts that surprise people:

- **BEAM-dependent tests skip themselves** when `erlc` is not on
  `PATH`, so a green `cargo test` does not by itself mean the codegen
  and replay tests ran. Set `HIRD_REQUIRE_BEAM=1` to turn those skips
  into failures, which is what the CI job with Erlang installed does.
- **Snapshot tests** use `insta`. When a change is *meant* to move a
  snapshot, re-run the tests with `INSTA_UPDATE=always`, check the
  resulting diff, and commit the updated `.snap` files — leaving no
  `.snap.new` files behind. (`cargo insta review` works too, but the
  dev shell does not ship it.)
- **The audit wire format is pinned by goldens.** `conformance/v1/`
  holds the bytes, `docs/tool-effects.md` specifies them, and two
  implementations are held to them: the `wire` module of `hird-check`
  and `runtime/hird_types.erl`. If a change touches the format, read
  the stability policy in `docs/audit-evidence.md` first — it says
  which changes may reuse a `schema_version` directory and which need a
  new one.
- **New Rust files need the license header**, immediately followed by a
  blank line:

  ```rust
  // Copyright 2026 the Hird Authors
  // SPDX-License-Identifier: Apache-2.0 OR MIT
  ```

[`README.md`](README.md#repository-layout) has the repository layout;
`docs/` holds the normative specifications.

## Definition of done

A change is finished when:

- `cargo fmt` is clean and `cargo clippy` passes with `-D warnings`.
- Public API is documented. `missing_docs` is a warning workspace-wide,
  and the docs job denies rustdoc warnings on private items too.
- Tests are added or updated whenever behavior changes, and they are
  deterministic. Unit tests live next to the code they cover.
- No new dependency added to a core crate without a reason worth
  stating in the pull request. Examples and benchmarks live in their own
  top-level workspace crates, so extra dependencies never show up as
  dev-dependencies of a core crate.
- Foundational crates stay `#![no_std]` where practical (`hird-lex`,
  `hird-types`, and `hird-actors` are checked against
  `x86_64-unknown-none` in CI), with `std` behind an explicit feature.
- The diff contains only what the change needs. Adjacent cleanups,
  reformatting, and refactors of things that were not broken belong in
  their own commits, if anywhere.

## Commits and pull requests

Commit subjects are lowercase, imperative, and prefixed with the area
they touch — usually the crate's short name (`lex:`, `parse:`,
`check:`, `codegen:`, `cli:`, `lsp:`, `mcp:`) or one of `docs:`, `ci:`,
`flake:`, `runtime:`, `demo:`, `tree-sitter:`, `chore:`. No trailing
period. `git log` is the reference.

Context — why the change, what was rejected, what it unblocks — goes in
the commit message or the pull request body. It does not go in the
source tree: code, comments, and docstrings must not reference tickets,
plans, phases, or work sessions, because those rot in place while the
code around them changes.

Keep commits one logical change each, rebase onto `main` rather than
merging it in, and leave `.beads/` alone — see below.

## Issues, and how the roadmap works

The roadmap and the day-to-day task list live in an in-repo tracker
([Beads](https://github.com/gastownhall/beads), driven by the `bd`
CLI), whose history is a Dolt database in this repository's
`refs/dolt/data` ref. Writing to it needs push access, so it is
maintainer-facing by construction. `.beads/README.md` documents that
workflow; it is not something a contribution needs to touch, and pull
requests should not include changes under `.beads/`.

From outside, use GitHub:

- **[Issues](https://github.com/no-materials/hird/issues)** for bugs
  and concrete proposals. Accepted ones get mirrored into the tracker
  by a maintainer and linked back — the GitHub issue stays the place
  for the conversation, so you do not need `bd` to follow along.
- **[Discussions](https://github.com/no-materials/hird/discussions)**
  for questions, design ideas, and anything not yet shaped like a task.

A useful bug report has: the `hird --version` output (or the commit you
built), your OS, the Erlang/OTP version if the BEAM is involved, the
smallest `.hird` source that reproduces it, and the exact command and
its full output. For a wrong-behavior report, the recorded audit log
(`hird run … --audit-file run.jsonl`) is worth more than a description:
it replays.

## License

Hirð is dual-licensed under Apache-2.0 or MIT, at your option. Unless
you state otherwise, any contribution you intentionally submit for
inclusion in the work, as defined in the Apache-2.0 license, is dual
licensed as above, without any additional terms or conditions.

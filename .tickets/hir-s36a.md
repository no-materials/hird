---
id: hir-s36a
status: closed
deps: []
links: []
created: 2026-07-29T13:42:23Z
type: task
priority: 2
assignee: nomaterials
tags: [tooling, editor, treesitter]
---
# Tree-sitter grammar and highlight queries for .hird

Hirð buffers render unhighlighted in editors: nothing ships a
tree-sitter grammar or syntax file for .hird, so the LSP's hover,
diagnostics, and go-to-definition sit next to plain monochrome text.

Write a tree-sitter grammar for the v0.1 surface, with highlight
queries, and expose it so editor configs can consume it the same way
they already consume hird-lsp (a flake output).

The normative grammar is docs/grammar.md, which is in line with the
implemented parser. Highlighting is the goal; the grammar does not
need to be error-recovery-perfect, but it must parse all shipped
sources cleanly.

Scope:

- Grammar covering every v0.1 declaration and expression form:
  module/use, fn, type (opaque, constructors), effect, tool, extern,
  actor (state/message/init/handle members), supervisor, let/lambda/
  if/match/handle/install, spawn/supervise/child/send/request/reply,
  crash!/panic!, records, tuples, lists, effect-row annotations.
- Both operator forms per the lexer's Unicode canonicalization: ->
  and →, => and ⇒, \ and λ lex identically and must parse
  identically.
- Query files: highlights.scm at minimum (keywords, types,
  constructors, effects and tool markers, strings, numbers, comments,
  operators); indents.scm and folds.scm if cheap.
- A flake package output (pkgs.tree-sitter.buildGrammar) so nvf/
  nvim-treesitter configs can take it as an input, next to the
  existing hird-lsp package output.
- Decide placement: in-repo directory (e.g. tree-sitter-hird/) versus
  a separate repo. In-repo is the default unless nvim-treesitter
  upstreaming forces a split; record the choice in the ticket on
  close.

## Acceptance Criteria

- The grammar parses demo/agent_planner.hird and every .hird test
  fixture in the repo with zero ERROR nodes.
- ASCII and Unicode operator spellings produce identical parses.
- highlights.scm distinguishes keywords, type/constructor names,
  effect and tool names, strings, numbers, and comments.
- The flake exposes the grammar as a package output; a note in the
  README's editor section shows how to wire it into nvim-treesitter.
- Building the grammar and running its tests is reproducible via nix
  (no global tree-sitter CLI assumed).


## Notes

**2026-07-31T08:40:35Z**

Placement: in-repo, at `tree-sitter-hird/`. Nothing forced a split — the
grammar tracks `docs/grammar.md` and the reference parser, so keeping it
in the same tree is what stops it drifting. Upstreaming to
nvim-treesitter would need its own repo; revisit only then.

`src/parser.c` and friends are generated, not committed: the flake package
uses `buildGrammar { generate = true; }`, so nix regenerates from
`grammar.js`. Only the hand-written `src/scanner.c` (nested block
comments, which no regular token can express) is tracked.

Note on scope: `=>` / `⇒` (FatArrow) is a lexer token with no production
anywhere in the v0.1 grammar, so there is nothing for either spelling to
parse into and no token was added for it. `->`/`→`, `\`/`λ`, `&&`/`∧` and
`||`/`∨` all parse identically, with corpus tests asserting the trees
match.

Not wired into `.github/workflows/ci.yml` — CI has no nix step today, and
adding one is a separate call. `nix flake check` runs the grammar checks
locally.

**2026-07-31T09:29:28Z**

Correction to the note above: it is wired into CI after all. `.github/
workflows/ci.yml` gained a `flake` job that installs nix and runs
`nix flake check` (which runs `checks.tree-sitter-hird`) plus
`nix build .#tree-sitter-hird` — `flake check` only evaluates package
outputs, so the package the editor configs consume is built explicitly.

First nix job in this CI; Linux only, since none of the flake's outputs
are platform specific.

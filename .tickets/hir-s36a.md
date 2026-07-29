---
id: hir-s36a
status: open
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


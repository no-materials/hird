---
id: hir-kw4v
status: closed
deps: []
links: [hir-0s3s]
created: 2026-06-17T12:57:48Z
type: task
priority: 1
assignee: nomaterials
parent: hir-89zs
tags: [phase-3, parser, modules, imports]
---
# Surface syntax for selective and aliased imports

Add surface syntax for the use-import forms the module system (hir-i0u7)
needs: whole-module, aliased, and selective imports. Today the parser accepts
only a `::`-separated path with an optional `as` alias and has no selective
form, while the canonical surface syntax (phrasebook.md) uses dot separators and
a brace-group selective list.

This ticket is grammar-only: lexer, parser, and AST projection. Import
resolution, qualified-name disambiguation, visibility, and collision checks all
land in hir-i0u7.

## Design

Match the canonical phrasebook forms (phrasebook.md is authoritative for
surface syntax), which are dot-separated and Gleam-style:

  use Ets                  whole-module import
  use Log as L             aliased import
  use Ets.{Table, lookup}  selective import (members brought in unqualified)

Decisions:
- Use the dot `.` as the path/member separator, replacing the current `::` in
  use paths, to match the phrasebook and the qualified-name form `Mod.member`.
- A use target is one or more PascalCase segments separated by `.`, an optional
  `.{ name, name, ... }` selective group, and an optional `as Alias`.
- Selective and aliased forms are mutually exclusive on one use (no
  `use M.{a} as X`).

Layers to touch:
- hird-lex: no new tokens expected (`.` `{` `}` `,` and contextual `as` already
  lex); confirm.
- hird-parse: extend the use-decl/path parser to dot separators and the
  `.{ ... }` selective group; keep `as Alias`. Tailored diagnostic on a
  malformed group.
- hird-ast: add an accessor on UseDecl for the selected member names, keeping
  the existing path() and alias().

Non-goals (owned by hir-i0u7):
- Resolving imports against module exports.
- Disambiguating a qualified name `Mod.member` from record field access `val.x`
  (a check-side rule keyed on whether the receiver resolves to a module).
- Visibility and duplicate/collision diagnostics.

## Acceptance Criteria

- `use Ets` parses (whole-module).
- `use Log as L` parses; alias() == "L".
- `use Ets.{Table, lookup}` parses; the selected names project as ["Table", "lookup"].
- A dot-separated path `use A.B` parses into segments ["A", "B"].
- A malformed selective group (`use Ets.{}` or `use Ets.{,}`) is a parse error with a helpful message.
- CST/snapshot tests cover whole-module, aliased, selective, and the error case.
- fmt, clippy (-D warnings), and tests pass for hird-lex, hird-parse, hird-ast.


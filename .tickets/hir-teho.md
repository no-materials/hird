---
id: hir-teho
status: closed
deps: []
links: [hir-0s3s]
created: 2026-06-17T12:10:26Z
type: task
priority: 1
assignee: nomaterials
parent: hir-89zs
tags: [phase-3, parser, types, modules]
---
# Surface syntax for the opaque type modifier

Add surface syntax for declaring opaque types, so the module system
(hir-i0u7) can distinguish transparent type exports from opaque ones.

An opaque type exports its name but not its constructors: other modules may
hold, pass, and store values of the type, but cannot construct them or
pattern-match them. This is the foundation for capability types (Table, Tool,
Db, Clock, Random, Log per ADR-006) and for user-defined abstract data types
that enforce invariants (e.g. a validated Email that can only be built by its
module parse function).

This ticket is grammar-only: lexer, parser, and AST projection. The semantic
enforcement (declaring-module tracking, cross-module construct/destructure
errors) lands in hir-i0u7.

## Design

Follow the Gleam three-level model (consistent with the
conventions locked in OD6 / hir-0s3s):

  type Foo = ...             private: name + constructors module-only
  pub type Foo = ...         transparent: name + constructors exported
  pub opaque type Foo = ...  opaque: name exported, constructors module-private

`opaque` is a modifier that only follows `pub`: a private type is already
module-only, so opaque adds nothing there. `pub type` stays transparent by
default; opacity is opt-in via `pub opaque type`. This matches Gleam and the
principle of least surprise, and keeps a plain `pub type` exporting its
constructors exactly as it does today.

Layers to touch:
- hird-lex: add an `opaque` keyword token, mirroring how `pub` and `type` are
  lexed.
- hird-parse: accept `opaque` between `pub` and `type` in a type declaration
  and attach it to the TYPE_DECL node; report `opaque` without `pub` (or
  without `type`) with a tailored diagnostic.
- hird-ast: add `TypeDecl::is_opaque()` alongside the existing `is_pub()`.

Example surface:

  module Email
  pub opaque type Email = Email(String)
  pub fn parse(raw: String) -> Option<Email> =
    if is_valid(raw) then Some(Email(raw)) else None

Non-goals (owned by hir-i0u7): tracking the declaring module, and emitting the
cannot-construct / cannot-destructure-outside-module errors.

## Acceptance Criteria

- `pub opaque type Foo = Bar(Int)` parses; the TYPE_DECL projects is_opaque() == true, is_pub() == true.
- `pub type Foo = Bar(Int)` parses; is_opaque() == false, is_pub() == true.
- `type Foo = Bar(Int)` parses; is_opaque() == false, is_pub() == false.
- `opaque type Foo = ...` (opaque without pub) is a parse error with a helpful message.
- CST/snapshot tests cover all three valid forms plus the error case.
- fmt, clippy (-D warnings), and tests pass for hird-lex, hird-parse, hird-ast.


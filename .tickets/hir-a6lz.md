---
id: hir-a6lz
status: closed
deps: [hir-ee8k]
links: []
created: 2026-05-22T21:38:22Z
type: task
priority: 2
assignee: nomaterials
parent: hir-0rzf
tags: [phase-4, ir, testing]
---
# IR pretty-printer and round-trip tests

Implement the IR pretty-printer and the round-trip property test.

**Pretty-printer**:
- Takes IR and emits Hirð source code in canonical form.
- Output is syntactically valid and parseable.
- Indentation and formatting follow canonical style (to be defined).
- Type annotations are included on all top-level declarations.
- Effect annotations are included where present (empty in pre-Phase 5 IR).

**Round-trip property**:
The core correctness property: for any well-typed program,
  source -> parse -> infer -> lower_to_IR -> pretty_print -> parse -> infer -> lower_to_IR
produces an IR that is structurally equivalent to the first IR (modulo source spans).

This property test catches:
- Pretty-printer bugs (output doesn't parse).
- Lowering bugs (information lost in lowering).
- Inference instability (re-inference produces different types).

Implement as:
1. A set of hand-written programs that exercise all IR node kinds.
2. A proptest generator for well-typed programs (reuse from Phase 3 if available).
3. The round-trip assertion as a reusable test harness.

This property becomes the regression safety net for all future compiler changes.

## Acceptance Criteria

- Pretty-printer emits valid, parseable Hirð source from IR.
- Round-trip property holds for at least 10 hand-written programs.
- proptest round-trip runs for randomly generated programs (at least 100 cases).
- Pretty-printer output matches canonical formatting style.
- Snapshot tests for pretty-printer output on representative programs.


## Notes

**2026-06-22T10:51:05Z**

Landed in hird-ir. pretty_print(&IrModule) -> String (re-exported) renders
canonical Hirð source; tests/roundtrip.rs proves the round-trip property.

Pretty-printer:
- Canonical form: a `module <Name>` header, then one declaration per
  blank-line-separated block, each on a single logical line. Unicode operator
  forms throughout (→ λ ∧ ∨). Minimal parentheses via a precedence ladder
  (left-assoc operators keep their left operand bare; the non-associative
  relational tier parenthesises both; function/field positions wrap lower-prec
  callees/receivers). Match scrutinees need no parens — the parser excludes `{`
  from application args, so the arms' brace always self-delimits.
- Desugarings are surfaced, not reversed (IR is the canonical form): a lowered
  `if` prints as the `match` over Bool it became; a handle prints as its body;
  operators print infix.
- Signatures print every param type and, where expressible, the return type.
  The return annotation is omitted when the type has no surface syntax — a
  record, unit (), or a zero-ary function `() → T` (which would re-parse as the
  one-arg `(()) → T`) — and inference recovers it. The empty effect row is
  elided. Externs synthesise param names (p0, …) and always print their
  required return type.
- Per-signature, type-variable letters are renumbered to a, b, c… by first
  appearance, so output does not depend on the unification-variable ids
  inference assigned. Skolems (lowercase TyCon) and unification vars are both
  treated as variables; type-def fields render under their declared param
  names (no renumbering).

Round-trip property (tests/roundtrip.rs): source → check → lower → pretty_print
→ check → lower reproduces the first IR up to type-variable renaming. Equality
is taken modulo renaming via a per-declaration normalisation that renumbers
unification vars AND skolems (annotating a return type moves a fn onto the
checker's rigid-skolem path, so an inferred TyVar and a skolem TyCon denote the
same variable); type declarations are compared verbatim. 15 hand-written
programs cover every node kind, both desugarings, operator precedence, and the
inexpressible-return cases; a proptest generator (well-typed by construction,
type-directed) runs 256 cases (stressed to 1000×3). Three pretty-printer
snapshots pin the canonical formatting.

Edge case found and fixed mid-implementation: a function returning a zero-ary
function value (e.g. `fn get() = answer` where `answer : () → Int`) has return
type `() → Int`, which is unwritable — emitting it re-parsed as `(()) → Int`
and failed re-check. is_expressible now rejects empty-param function types.

docs/ir.md gains "Pretty-printing" and "Round-trip property" sections. fmt +
clippy (-D warnings) + full workspace tests pass.

---
id: hir-89zs
status: open
deps: [hir-vm5s]
links: []
created: 2026-05-22T21:32:36Z
type: epic
priority: 0
assignee: nomaterials
tags: [phase-3, types]
---
# Phase 3 — Type System Core

## Goal

Implement Hindley-Milner type inference with algebraic data types, modules,
and opaque type constructors for capabilities. This phase builds the foundation
that effect rows (Phase 5) and actor types (Phase 7) extend.

## v0.1 demo relevance

The planner demo requires type-checked function definitions, ADT message types,
pattern match exhaustiveness on those message types, and opaque capability types
for tool handles (Table, Db, Http, Tool, Clock, Random, Log). Without this
phase, there is no type checking at all.

## Design context

Type inference is Hindley-Milner with let-polymorphism. The implementation
should be written from scratch — the combination of row polymorphism (added
in Phase 5), opaque capabilities, and BEAM-specific constraints makes library
reuse impractical. Study Gleam's `compiler-core` for reference patterns, but
expect significant divergence.

Key design decisions in this phase:

- **ADT declarations** follow ML-style: `type Option<A> = Some(A) | None`.
  Constructors are functions. Destructuring is via pattern match.
- **Pattern match exhaustiveness checking** is required. Missing cases are
  compile errors, not warnings. This is non-negotiable for actor message
  handlers where missing a message variant is a correctness bug.
- **Opaque type constructors** (`Table<K, V, Perm>`, `Tool<Name, Input, Output>`,
  etc.) are declared with `type` but their internals are not exposed outside
  the declaring module. Operations on opaque types must go through declared
  functions with appropriate effect signatures (added in Phase 5).
- **Module system**: modules with explicit `use` imports and export lists,
  following Gleam-style conventions. Qualified names (`Mod.func`) for
  disambiguation. Module resolution is file-based (one module per file) unless
  a compelling reason to deviate emerges.

## Task sequence

1. [x] [hir-jj3l](hir-jj3l.md) — Type representation and unification engine
2. [x] [hir-h8qo](hir-h8qo.md) — Complete the hird-ast projection: type expressions and patterns
3. [x] [hir-lhyh](hir-lhyh.md) — Let-polymorphism and ADT type checking
4. [x] [hir-n3si](hir-n3si.md) — Pattern match exhaustiveness checking
5. [ ] [hir-i0u7](hir-i0u7.md) — Module system and opaque types

Steps 4 and 5 are independent after step 3.

## Open design question

- **OD6 (Module and visibility system)**: default to Gleam-style `use`
  imports with explicit export lists. Flag any deviation during implementation.

## Out of scope

- Effect rows and effect inference (Phase 5).
- Actor type semantics (Phase 7).
- Row polymorphism for records (deferred — may be added with effect rows).
- Type classes, traits, or overloading (not in v0.1; possibly never).
- Dependent types or refinement types (not in v0.1; Cure occupies that space).

## Acceptance Criteria

- Type representation covers: type variables, type constructors (named, applied),
  function types, tuple types, record types (structural), ADT instances.
- Unification with occurs check, producing clear error messages on infinite types.
- Let-polymorphism: polymorphic let-bound values generalize correctly; lambda-bound
  values are monomorphic.
- ADT declarations type-check: constructors are typed functions, type parameters
  propagate correctly, recursive types work.
- Pattern match exhaustiveness: missing constructors produce compile errors with
  the list of unhandled cases. Redundant patterns produce warnings.
- Module system: `use` imports resolve, qualified names work, export lists control
  visibility. Circular imports are detected and rejected.
- Opaque type constructors: `type Table<K, V, Perm>` can be declared, constructed
  inside its module, and used opaquely outside. Attempting to destructure an opaque
  type outside its module is a compile error.
- Type error diagnostics: "expected X, got Y" with source spans, showing both the
  expected and actual type in readable form. At least 10 distinct error patterns
  have snapshot tests.
- Property tests: randomly generated well-typed expressions type-check successfully;
  randomly generated ill-typed expressions produce errors (not panics).
- `cargo clippy` and `cargo test` pass for `hird-types`.


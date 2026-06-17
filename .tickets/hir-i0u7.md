---
id: hir-i0u7
status: open
deps: [hir-lhyh, hir-teho]
links: [hir-0s3s]
created: 2026-05-22T21:37:55Z
type: task
priority: 1
assignee: nomaterials
parent: hir-89zs
tags: [phase-3, types, modules]
---
# Module system and opaque types

Implement the module system with scope resolution, imports, exports, and opaque
type constructors.

**Module system**:
- One module per file. Module name derived from file path (configurable).
- `use` imports: `use Mod` (import all public names), `use Mod.{foo, bar}`
  (selective), `use Mod as M` (aliased).
- Export lists: `pub fn`, `pub type`. Unprefixed declarations are module-private.
- Qualified names: `Mod.func` for disambiguation.
- Circular import detection: build a module dependency graph; cycles are errors.
- Module resolution order: local modules first, then standard library paths.

**Opaque type constructors**:
- `type Table<K, V, Perm>` declared in a module can be constructed inside that
  module but is opaque outside it.
- External code can hold values of opaque types, pass them around, and use them
  as arguments to functions that accept them — but cannot inspect or destructure them.
- Attempting to pattern match on an opaque type outside its declaring module is
  a compile error: "cannot destructure opaque type Table outside module Ets".
- This is the foundation for the capability discipline: Tool, Db, Http, Clock,
  Random, Log types will all be opaque capabilities.

**Open design question (OD6)**: default to Gleam-style use/export. Flag if
the implementation suggests a deviation.

## Acceptance Criteria

- use imports resolve correctly: selective, aliased, and wildcard.
- Export lists control visibility: private names are inaccessible from other modules.
- Qualified names work: Mod.func resolves to the correct definition.
- Circular imports detected and reported as errors.
- Opaque types: construction inside declaring module works; destructuring outside
  is a compile error with a clear message.
- Opaque types can be passed as arguments and stored in data structures.
- Snapshot tests for: imports, qualified names, visibility errors, circular
  imports, opaque type construction, opaque type destructure error.
- At least 10 snapshot tests.


## Notes

**2026-06-01T13:54:44Z**

Inbound from hir-of12: duplicate / conflicting top-level definition detection lands HERE (this
ticket owns scope resolution and name binding). It was pulled out of the parser ticket (hir-of12)
because a complete check must span the module system, not a single file.

Cover the collision cases that only make sense with modules:
- two top-level definitions sharing a name within a module,
- an import that collides with a local definition,
- import-vs-import collisions.
Emit a clear diagnostic carrying both spans (original definition + redefinition). Namespace rules
(e.g. whether `fn foo` and `type Foo` share a namespace) are an OD6 design call — pin them in
hir-0s3s. Add snapshot tests for the duplicate/collision cases alongside the import/visibility tests.

Note: hir-lhyh's "shadowing (warn, don't error)" is a DIFFERENT concern (inner let-bindings
shadowing outer scopes), not top-level duplicate definitions.

**2026-06-17T12:11:07Z**

Fork A (opaque-type mechanism) — locked.

**Decision: explicit `opaque` modifier, Gleam-style three-level visibility.**

  type Foo = ...             private (module-only)
  pub type Foo = ...         transparent (name + constructors exported)
  pub opaque type Foo = ...  opaque (name exported, constructors module-private)

`pub type` stays transparent by default; opacity is opt-in via `pub opaque
type`. Rationale: matches the Gleam-style conventions OD6 (hir-0s3s)
already commits to, satisfies the Explicit-Over-Implicit tenet, and lets users
define their own invariant-enforcing abstract types — not just the built-in
capabilities. Capability types (Table, Tool, Db, Clock, Random, Log per
ADR-006) are opaque types under this same mechanism: opacity is exactly what
makes a capability unforgeable (its constructor is private to the declaring
module, so no other code can mint or upgrade one).

**Grammar precursor: hir-teho** (this ticket now depends on it). It adds the
`opaque` keyword to the lexer, parses `pub opaque type`, and exposes
`TypeDecl::is_opaque()` next to `is_pub()`. Grammar-only; no semantics.

**What stays in this ticket (the semantic half):**
- The registry records, per constructor, its declaring module and whether the
  owning type is opaque.
- Constructing an opaque type (`Foo(..)` as a value) outside its declaring
  module is an error.
- Destructuring an opaque type outside its declaring module is an error. This
  plugs into the constructor-pattern path added by hir-n3si: when a pattern
  names a constructor of an opaque type defined elsewhere, emit a dedicated
  "cannot destructure opaque type `Foo` outside module `Bar`" diagnostic
  instead of the generic unknown-constructor error.
- Inside the declaring module, opaque types behave as ordinary ADTs (full
  construction and pattern matching).

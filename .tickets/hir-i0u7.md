---
id: hir-i0u7
status: closed
deps: [hir-lhyh, hir-teho, hir-kw4v]
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

**2026-06-17T12:58:16Z**

Forks B–D — locked (decisions are the recommended ones).

**B — Import syntax: dot separator + selective grammar (precursor: hir-kw4v).**
Adopt the phrasebook forms (phrasebook.md is authoritative for surface syntax):
`use Mod`, `use Mod as M`, `use Mod.{a, b}`, with `.` as the separator — the
current `::` is the bug. The selective/aliased grammar lands in hir-kw4v; this
ticket owns RESOLUTION: binding selective names unqualified, binding the
module/alias for qualified access, and resolving `Mod.member`. Qualified name
vs record field access (`Ets.lookup` vs `point.x`) is disambiguated check-side
by the receiver: a bare PascalCase name that resolves in the module namespace is
a qualified name; otherwise it is field access on a value. The casing convention
guarantees the two never overlap.

**C — Whole-program driver contract.**
hird-check gains a program-level entry, roughly
`check_program(modules: &[(ModuleName, SourceFile)]) -> CheckedProgram`, which
builds the use-graph, rejects cycles (reuse the existing Tarjan SCC code in
checker.rs), checks modules in dependency order, and seeds each module's
environment from its imports' EXPORTED schemes. A thin driver (the CLI, or a
small session layer) does file discovery + parse and hands parsed modules in;
the single-file `check()` stays as the per-module core. Module names are
path-derived and validated against the file's `module` declaration (mismatch =
error). Standard-library resolution is deferred for v0.1 behind a seam (local
modules only).

**D — Two namespaces (types vs values).**
Types and values occupy separate namespaces, so `type Email = Email(String)`
(type `Email` plus constructor `Email`) is legal — required for opaque
capability types. The registry already reflects this (separate `adts` and
`ctors` maps). Duplicate detection runs per namespace:
- type vs type (same name) -> error; constructor vs constructor -> error;
  value vs value (fn or binding) -> error;
- type vs value -> NOT a collision (different namespaces);
- fn vs constructor -> impossible (casing forbids the textual clash).
Import collisions follow the same per-namespace rule (import-vs-local,
import-vs-import). Every duplicate diagnostic carries both spans (original
definition + redefinition), per the hir-of12 inbound scope.

**2026-06-18T14:56:03Z**

Landed in hird-check. The single-file check() stays the per-module core; a new
program-level check_program(modules: &[(ModuleName, SourceFile)]) -> CheckedProgram
wraps it: path-derived module names validated against the `module` decl (C0019),
import use-graph condensed with the existing Tarjan SCC code, cycles rejected
(C0020), modules checked callees-first with each seeded from its imports'
exported schemes. Stdlib resolution deferred behind a local-only seam
(unresolved import = C0023).

Imports: whole-module `use Mod` and `use Mod as M` bind a qualifier (trailing
segment or alias) for `Mod.member` access; `use Mod.{a, b}` binds members
unqualified. Qualified name vs field access disambiguated by a PascalCase
receiver resolving in the module namespace (C0024 for an unknown qualified
member).

Visibility: pub fn / pub type / pub opaque type drive the export interface;
unprefixed is private. Opaque types record their declaring module + opacity per
constructor; construct (C0022) or destructure (C0021) outside the declaring
module is an error naming the type and module, while inside it they are ordinary
ADTs. Opaque values pass and store freely.

Two namespaces (D): per-namespace duplicate detection — value-namespace dup
(C0017: fn/extern/ctor, incl. import-vs-local and import-vs-import) and
type-namespace dup (C0018); `type Email = Email(String)` is legal. Every
duplicate carries both spans via a new RelatedSpan on CheckDiagnostic.

17 module snapshot tests (tests/modules.rs) cover selective/aliased/wildcard
imports, qualified names, visibility, circular imports, opaque construct/
destructure/round-trip, and the collision cases. fmt + clippy (-D warnings) +
full workspace tests pass. OD6 promoted to ADR-010; its open-decision row
removed.

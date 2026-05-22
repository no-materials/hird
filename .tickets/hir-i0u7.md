---
id: hir-i0u7
status: open
deps: [hir-lhyh]
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


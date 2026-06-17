---
id: hir-0s3s
status: open
deps: []
links: [hir-i0u7, hir-teho, hir-kw4v]
created: 2026-05-22T21:43:31Z
type: task
priority: 2
assignee: nomaterials
tags: [decision, design, modules]
---
# OD6: Module and visibility system

Resolve the module and visibility system design.

**Resolution**: Gleam-style use/export.

- Modules with explicit use imports: use Mod, use Mod.{foo, bar}, use Mod as M.
- Export via pub keyword: pub fn, pub type. Unprefixed is private.
- One module per file. Module name derived from file path.
- Qualified names: Mod.func for disambiguation.
- No first-class modules, no functors, no module-level type abstraction beyond
  opaque types.

This follows the conventions of an LLM-first sibling project, for consistency
and simplicity. The module system is intentionally simple.

**Decision point**: Phase 3 implementation.

## Acceptance Criteria

- Decision documented in DECISIONS.md.
- Module system implements Gleam-style use/export.
- Any deviation from those conventions flagged and documented.


## Notes

**2026-06-17T12:11:07Z**

Opacity in the visibility model (resolves fork A raised in hir-i0u7).

Three visibility levels for type declarations:

  type Foo = ...             private (module-only)
  pub type Foo = ...         transparent: name + constructors exported
  pub opaque type Foo = ...  opaque: name exported, constructors module-private

`opaque` is an opt-in modifier on `pub` (Gleam parity; a private type gains
nothing from it). Opaque types are the mechanism behind capability types
(Table, Tool, Db, Clock, Random, Log per ADR-006): construction and
destructuring are confined to the declaring module, which is what makes a
capability unforgeable. Grammar lands in hir-teho; semantic enforcement in
hir-i0u7. Fold this three-level model into the DECISIONS.md entry when OD6 is
promoted (per this ticket's acceptance criteria).

**2026-06-17T12:58:16Z**

E — Promote OD6 to an accepted decision in DECISIONS.md.

When the module work lands, add a new accepted entry (ADR-010; the file
currently stops at ADR-009) and remove the OD6 row from the "Open Decision
Slots" table. The entry consolidates the resolved module + visibility
decisions:
- one module per file; path-derived module name, validated against the `module`
  declaration;
- imports `use Mod` / `use Mod as M` / `use Mod.{a, b}`, dot separator;
  qualified access `Mod.member` disambiguated from field access by a PascalCase
  receiver;
- three type-visibility levels: private / `pub` (transparent) / `pub opaque`;
- separate namespaces for types and values (so `type Email = Email(String)` is
  legal);
- circular imports detected and rejected.
ADR-010 references ADR-006 (opaque-capability discipline), which relies on this
opaque-type mechanism, rather than restating it. This satisfies this ticket's
"decision documented in DECISIONS.md" acceptance criterion. Sequencing: write it
last, once the above are implemented and validated.

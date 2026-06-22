---
id: hir-ee8k
status: closed
deps: [hir-i0u7]
links: []
created: 2026-05-22T21:38:12Z
type: task
priority: 1
assignee: nomaterials
parent: hir-0rzf
tags: [phase-4, ir]
---
# IR data structures and lowering

Define the IR data structures and implement lowering from the typed AST.

**IR node kinds** (each carries an explicit, fully-resolved type):
- IrLet { name, type, value, body }
- IrLambda { param, param_type, body, body_type }
- IrApp { func, arg, result_type }
- IrMatch { scrutinee, scrutinee_type, arms: Vec<(Pattern, IrExpr)>, result_type }
- IrConstructor { name, type_name, args, result_type }
- IrLiteral { value, type }
- IrVar { name, type }
- IrModule { name, declarations }
- IrFnDef { name, params, return_type, effect_row (initially empty), body }
- IrTypeDef { name, params, constructors }
- IrExternRef { name, type, module }

**Key properties**:
- Every node has an explicit type (no unresolved type variables).
- Types are fully substituted and readable.
- Effect rows are present as empty placeholders (filled by Phase 5).
- The IR is structurally simpler than the AST: syntactic sugar is desugared,
  operator expressions are lowered to function application, etc.

**Lowering pass** (AST -> IR):
- Traverse the typed AST post-inference.
- Apply the final substitution to resolve all type variables.
- Desugar: if-then-else to match, operator expressions to applications,
  multi-argument functions to curried lambdas (or not — design choice).
- Produce fully-typed IR nodes.

**JSON serialization**:
- IR fragments serialize to JSON for MCP server consumption.
- Use serde with a stable, documented schema.

## Acceptance Criteria

- IR data structures defined in hird-ir with all node kinds listed.
- Every IR node carries an explicit, resolved type.
- Lowering from typed AST produces IR for: let, lambda, application, match,
  constructors, literals, variables, modules, function defs, type defs, externs.
- Desugaring: if-then-else lowered to match, operators lowered to application.
- JSON serialization via serde produces stable, documented output.
- Unit tests: lower a typed AST for at least 5 distinct programs, verify IR structure.
- docs/ir.md documents IR node kinds, fields, and JSON schema.


## Notes

**2026-06-19T13:23:23Z**

Landed in hird-ir. lower_module(file, checked, name) walks the typed AST — the
CheckedFile per-node type side-table, already substitution-resolved by the
checker — and emits fully-typed IR; no unification happens in lowering.

Node kinds: IrModule, IrDecl (Fn/Type/Extern), IrFnDef (with an empty EffectRow
placeholder), IrTypeDef, IrConstructorDef, IrExternRef; IrExpr (Let, Lambda,
App, Match, Constructor, Literal, Var, Tuple, List, Record, Field); IrPattern
(Constructor, Tuple, Literal, Wildcard, Bind). Every node carries its resolved
hird_types::Type.

Design decisions:
- N-ary functions and applications, not curried. This matches the n-ary type
  system (TyFn(params, ret); (A,B)->C distinct from A->(B->C)) and the BEAM
  target. The ticket left this open ("curried lambdas (or not)"); n-ary chosen.
- Desugaring: if -> match over Bool (synthetic True/False constructor patterns);
  binary operators -> application of a primitive operator reference (logical
  ops canonicalised to the Unicode forms); parentheses dropped; handle -> its
  handled body (arms reference effects, which the IR gains in Phase 5, matching
  how the checker types a handle).
- Constructors keep their own IrConstructor node (PascalCase callee/name by the
  naming convention); type_name is the head constructor of the result type.
- Qualified names (Mod.member) lower to an IrVar with the dotted name, detected
  by a bare-name receiver the checker never typed as a value.
- Constructor field types in a type def render under the declared parameter
  names (Some -> "a", Cons -> "a", "List<a>").

JSON (serde Serialize; crate stays no_std + alloc via serde_json's alloc
feature): node enums are internally tagged with "kind"; embedded types render
as canonical strings ("List<Int>", "a -> b") for readable MCP/LLM output rather
than a nested type tree; literals carry source text; the empty effect row is {}.
Serialization is one-directional (no Deserialize). Schema documented in
docs/ir.md with a worked example.

Dependencies trimmed to hird-ast/hird-parse/hird-types/hird-check + serde/
serde_json; dropped the unused hird-effects/hird-actors scaffold deps (effects
and actors are out of scope for this epic).

11 tests (tests/lowering.rs) lower distinct programs and verify IR structure
across every node kind and desugaring, plus an exact compact-JSON check and a
pretty-JSON snapshot. fmt + clippy (-D warnings) + full workspace tests pass.

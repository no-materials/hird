---
id: hir-ee8k
status: open
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


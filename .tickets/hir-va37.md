---
id: hir-va37
status: open
deps: [hir-ed3z]
links: []
created: 2026-05-22T21:36:45Z
type: task
priority: 1
assignee: nomaterials
parent: hir-vm5s
tags: [phase-2, parser]
---
# Core syntax parsing

Implement chumsky parser productions for all v0.1 surface syntax, producing
rowan CST nodes and typed AST projections.

Productions to implement:

- **Modules**: module declaration with name, use-imports with optional aliasing,
  export lists.
- **Functions**: fn declaration with name, parameters with type annotations,
  return type, effect annotation (! { ... }), body expression.
- **Let bindings**: let name [: Type] = expr in body.
- **Lambdas**: λx -> body or \x -> body (both normalized by lexer).
- **Type declarations**: type Name<params> = Constructor1(fields) | Constructor2(fields).
- **Pattern matching**: match expr { Pattern1 -> body1, Pattern2 -> body2 }.
  Patterns: constructor patterns, variable bindings, wildcard, literal, tuple, nested.
- **Actor declarations**: actor Name { state: Type, message: Type, init: fn, handlers }.
- **Supervisor declarations**: supervisor Name { strategy, intensity, period, children }.
- **Effect declarations**: effect Name and effect Name<params>.
- **Tool declarations**: tool Name : InputType -> OutputType.
- **Handle blocks**: handle { EffectName -> impl_fn, ... } in body.
- **Extern declarations**: extern fn name(params) -> Type.
- **Expressions**: function application, binary operators (with precedence),
  if-then-else, record literals, tuple literals, list literals, field access.
- **Type expressions**: named types, applied types (T<A, B>), function types
  (A -> B), tuple types, effect-annotated function types (A -> B ! {E}).

Each production feeds into a typed AST node defined in hird-ast.

## Acceptance Criteria

- All productions listed above parse successfully from well-formed input.
- CST preserves all source tokens including whitespace and comments.
- Typed AST projection covers: LetExpr, Lambda, FnDecl, TypeDecl, MatchExpr,
  ModuleDecl, UseDecl, ExternDecl, EffectDecl, ActorDecl, SupervisorDecl,
  ToolDecl, HandleBlock, IfExpr, AppExpr, BinOpExpr, RecordLit, TupleLit.
- Operator precedence is correct for arithmetic, comparison, and logical ops.
- Snapshot tests for each syntax construct in isolation.
- At least one complete multi-declaration module parses successfully.


---
id: hir-zp13
status: open
deps: [hir-0bhk]
links: []
created: 2026-05-22T21:41:10Z
type: task
priority: 1
assignee: nomaterials
parent: hir-7rsf
tags: [phase-9, codegen, erlang]
---
# Erlang source emission from Glass IR

Implement the Erlang source code emitter: Glass IR → .erl files that compile
with stock erlc.

**Emission strategy**:
- One .erl file per Hirð module.
- One .erl file per actor (gen_server module, from Phase 7 codegen).
- One .erl file per supervisor (supervisor module, from Phase 8 codegen).
- A hird_runtime.erl support module providing shared runtime utilities.

**IR-to-Erlang mapping**:
- IrLet → variable binding (Erlang doesn't have let-in; map to case or match).
- IrLambda → Erlang fun(Param) -> Body end.
- IrApp → Function(Arg).
- IrMatch → Erlang case Expr of Pattern -> Body; ... end.
- IrConstructor → tagged tuple {constructor_name, Arg1, Arg2, ...}.
- IrLiteral → Erlang literal.
- IrVar → Erlang variable (capitalized: Hirð snake_case x_foo → Erlang X_foo).
- IrFnDef → Erlang function declaration.
- IrTypeDef → no runtime representation (types are erased); optionally emit a
  -type attribute for dialyzer compatibility.
- IrModule → Erlang -module(), -export() attributes.

**Variable naming**:
- Hirð uses snake_case; Erlang requires capitalized variables.
- Mapping: x → X, foo_bar → Foo_bar, or a systematic renaming scheme.
- Avoid collisions with Erlang reserved words.

**Effect handler wiring**:
- DI-style handlers lower to either:
  (a) Extra function parameters (handler map threaded through calls), or
  (b) Process dictionary storage (simpler codegen, less pure).
- This ticket implements the chosen strategy from Phase 5's handler ticket.

**Source readability**:
- Generated Erlang should be indented and formatted for human reading.
- Comments indicating the corresponding Hirð source location.
- Not beautiful, but inspectable.

## Acceptance Criteria

- All Glass IR node kinds emit valid Erlang.
- Generated .erl files compile with stock erlc without errors.
- Variable renaming avoids Erlang reserved word conflicts.
- ADT constructors map to tagged tuples.
- Pattern matching maps to Erlang case expressions.
- Lambdas map to Erlang funs.
- Effect handler wiring present in generated code.
- Source location comments in generated Erlang.
- Snapshot tests: generated Erlang for pure functions, effectful functions,
  pattern matching, ADTs, modules with exports.
- At least 10 snapshot tests of generated Erlang.


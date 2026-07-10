---
id: hir-zp13
status: closed
deps: [hir-0bhk]
links: []
created: 2026-05-22T21:41:10Z
type: task
priority: 1
assignee: nomaterials
parent: hir-7rsf
tags: [phase-9, codegen, erlang]
---
# Erlang source emission from IR

Implement the Erlang source code emitter: IR → .erl files that compile
with stock erlc.

**Emission strategy**:
- One .erl file per Hirð module.
- One .erl file per actor (gen_server module; emitted by hir-1dvq per the
  Phase 7 mapping ADR, ha-8fyg).
- One .erl file per supervisor (supervisor module, per Phase 8 codegen
  decisions).
- Runtime support modules (dispatcher, audit, handler registry) are
  hand-written by hir-7oph; this ticket emits *references* to them and never
  generates runtime code.

**Scope**: this ticket is plain modules plus every IR *expression* kind —
including spawn/send/request/reply, which lower to gen_server calls per
ADR-020 inside ordinary function bodies. IrDecl::Actor and
IrDecl::Supervisor behaviour-module shells are hir-1dvq and hir-z9rn
respectively. IrRequest's message builder is guaranteed to be a bare message
constructor by the checker (C0042), so its emission is total.

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

**Effect handler wiring** (per ADR-022):
- Functions with a non-empty or open effect row take one trailing
  handler-map parameter; pure functions keep surface arity. Lambdas follow
  the same type-directed rule; pure funs meeting effectful function types
  are eta-expanded.
- A handle block emits map extension over the in-scope map (or `#{}`);
  entries are normalised to binary funs `fun(Args, Handlers)`.
- Tool call sites emit `hird_tool_dispatch:call(tool_name, Handlers, Args)`
  — never a direct handler invocation — so audit capture is unconditional.
  Map keys: `{tool, marker}` for `Tool<Marker>`, head atom for bare effects.

**Source readability**:
- Generated Erlang should be indented and formatted for human reading.
- Declaration-level source comments (`%% <file>:<line>` above each form),
  per ADR-022. This requires adding a span field to IR declaration structs
  (serde-skipped) and populating it in lowering — in scope here.
  Expression-level mapping is v0.2 (abstract forms, ADR-002).
- Not beautiful, but inspectable.

## Acceptance Criteria

- All IR expression kinds emit valid Erlang (Actor/Supervisor declaration
  shells are hir-1dvq/hir-z9rn).
- Generated .erl files compile with stock erlc without errors.
- Variable renaming avoids Erlang reserved word conflicts.
- ADT constructors map to tagged tuples.
- Pattern matching maps to Erlang case expressions.
- Lambdas map to Erlang funs.
- Handler-map threading and tool-dispatch call sites emitted per ADR-022.
- Declaration-level source location comments in generated Erlang.
- Snapshot tests: generated Erlang for pure functions, effectful functions,
  pattern matching, ADTs, modules with exports.
- At least 10 snapshot tests of generated Erlang.


## Notes

**2026-07-09T12:59:51Z**

Also emits the crash! primitive: the IrExpr::Crash node (introduced by hir-0bhk) lowers to an Erlang exit (e.g. erlang:error/1). Add a crash! emission snapshot here. Supervisor behaviour-module emission is a separate ticket, hir-z9rn.

**2026-07-10T09:16:54Z**

Emission mechanics locked as ADR-022: single trailing handler-map parameter on effectful functions, tool calls route through hird_tool_dispatch:call/3, unhandled tools fall back to the runtime registry then crash, declaration-level source comments backed by new span fields on IR declaration structs. Body and acceptance criteria amended accordingly; hird_runtime.erl line removed (runtime modules are hir-7oph's, this ticket only references them).

**2026-07-10T09:56:10Z**

Implemented and closed. hird-codegen emits one .erl per module: all IR expression kinds, ADR-022 handler-map threading (trailing Handlers@ param on non-empty/open rows, handle blocks extend the in-scope map with binary-fun entries, tool calls route through hird_tool_dispatch:call/3, eta-expansion on convention mismatch), ADR-020 messaging (spawn -> start_link, send -> cast, request -> call/5000, reply -> gen_server:reply), crash! -> erlang:error/1, and declaration-level %% file:line comments backed by new serde-skipped IrSpan fields populated in lowering (IrLambda also gained its effect row for the type-directed convention). 17 erlc-validated snapshot tests; also verified end to end on BEAM against a stub dispatcher honouring the runtime contract (mocked tool + handle block returned the expected value). Actor/supervisor behaviour-module shells remain hir-1dvq/hir-z9rn; runtime library is hir-7oph. Commits: 880c917 (ir), bbeb764 (codegen).

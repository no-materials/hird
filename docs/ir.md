# Hirð IR

The IR is the fully-typed, structurally-simplified representation produced
after type inference. It is the contract between the compiler frontend and
every downstream consumer: codegen, LLM tooling, the MCP server, and
effect-graph analysis. It is designed for queryability first and codegen
convenience second.

Three properties hold for every IR produced by lowering a well-typed module:

- **Every node carries its resolved type.** Lowering reads back the types the
  checker recorded on each CST node; no node is left untyped, and no
  unification variables remain beyond those a polymorphic definition
  legitimately abstracts over.
- **Syntactic sugar is desugared.** `if` becomes `match`, operators become
  application, and parentheses are dropped.
- **The IR serializes to JSON.** Every node derives `serde::Serialize` with a
  stable, documented schema (below).

Effect rows are present on function definitions as empty placeholders; Phase 5
populates them. Actor and supervisor nodes are added in later phases.

## Lowering

`lower_module(file, checked, name)` turns a parsed `SourceFile` plus its
`CheckedFile` (the per-node type side-table) into an `IrModule`. The checker
has already applied the final substitution, so lowering does no unification —
it walks the CST and reads resolved types back by node identity.

Declarations that are resolved away or deferred to later phases — `use`
imports, `effect`/`tool` declarations, `actor`/`supervisor` declarations — do
not appear in the IR. Only functions, data types, and externs do.

### Desugaring decisions

| Surface                       | Lowers to                                              |
|-------------------------------|--------------------------------------------------------|
| `if c then a else b`          | `match c { True → a, False → b }` over `Bool`          |
| `a ⊕ b`                       | application of a primitive operator reference `IrVar`  |
| `(e)`                         | `e` (parentheses carry no semantics)                   |
| `handle { … } in body`        | `body` (handler arms reference effects; see Phase 5)   |

Operator references use the canonical operator symbol as the variable name
(`+`, `-`, `*`, `/`, `<`, `<=`, `>`, `>=`, `==`, `!=`, and the Unicode logical
forms `∧`, `∨`). The operator's type is the n-ary function type synthesised
from its operand and result types.

### N-ary functions

Functions and applications are **n-ary**, never curried: `f(a, b)` is a
two-argument application, and `λx y → e` is a two-parameter lambda. This
matches the type system (`(A, B) → C` and `A → (B → C)` are distinct types)
and the BEAM target, where functions take an argument list. The argument
shape follows the checker: a tuple-literal argument is the argument list
(`f(a, b)` is two arguments, `f()` is zero), and any other argument is a
single argument (`f((a, b))` passes one tuple).

## Node kinds

### Module and declarations

- `IrModule { name, declarations }` — a module's name and its declarations in
  source order.
- `IrFnDef { name, params, return_type, effect_row, body }` — a function. Each
  `IrParam { name, type }` is explicitly typed; `effect_row` is empty in this
  phase.
- `IrTypeDef { name, params, constructors }` — a data type. `params` are the
  declared type-parameter names; each `IrConstructorDef { name, fields }`
  lists its field types, with type-parameter variables rendered under their
  declared names (e.g. `a`, `List<a>`).
- `IrExternRef { name, type, module }` — a reference to an external function.
  `type` is the function's (possibly quantified) scheme. `module` names the
  backing foreign module; it is always absent in v0.1, where the surface
  syntax does not yet name one.

### Expressions

Every expression node carries its resolved type.

- `IrLet { name, type, value, body }` — `type` is the bound value's type. A
  polymorphic binding keeps its monomorphic value type here; each use site in
  the body carries its own instantiation.
- `IrLambda { params, body, body_type }`.
- `IrApp { func, args, result_type }`.
- `IrMatch { scrutinee, scrutinee_type, arms, result_type }`, where each
  `IrArm { pattern, body }`.
- `IrConstructor { name, type_name, args, result_type }` — a constructor
  applied to zero or more arguments. `type_name` is the data type it
  constructs. Nullary constructors (`None`, `True`) appear here with no
  arguments.
- `IrLiteral { value, type }` — `value` carries the literal's source text,
  tagged `Int`, `Float`, or `Str` (strings keep their surrounding quotes;
  integers keep full precision).
- `IrVar { name, type }` — a variable, function, operator, or qualified-name
  (`Mod.member`) reference. Its type is the instantiation at this use site.
- `IrTuple { elems, type }` — a tuple; an empty tuple is unit (`()`).
- `IrList { elems, type }`.
- `IrRecord { fields, type }`, where each `IrRecordField { label, value }`.
- `IrField { receiver, field, type }` — a record field access.

### Patterns

Every pattern node carries the type of the value it matches.

- `IrConstructorPat { name, type_name, fields, type }`.
- `IrTuplePat { elems, type }`.
- `IrLiteralPat { value, type }`.
- `IrWildcardPat { type }`.
- `IrBindPat { name, type }`.

## JSON schema

Lowered IR serializes through `IrModule::to_json()` (compact) and
`to_json_pretty()` (indented).

- **Node enums are internally tagged** with a `"kind"` field whose value is the
  node kind (`"Let"`, `"Match"`, `"Constructor"`, …). This applies to
  expressions, patterns, and declarations.
- **Types render as canonical strings.** Every `type`/`return_type`/
  `scrutinee_type`/`result_type`/`body_type` field is the type's canonical
  textual rendering (`"Int"`, `"List<Int>"`, `"a → b"`), not a nested type
  tree. This keeps the JSON readable for tooling and LLM consumption.
- **Literal values are tagged** by kind: `{"Int": "42"}`, `{"Float": "3.14"}`,
  `{"Str": "\"hello\""}`.
- **The empty effect row** serializes as `{}`.

Serialization is one-directional: the IR is produced by lowering and is not
parsed back from JSON.

### Example

The module

```
type Option<a> = Some(a) | None
fn unwrap(opt: Option<Int>) -> Int = match opt { Some(x) -> x, None -> 0, }
```

serializes (pretty) to:

```json
{
  "name": "Opt",
  "declarations": [
    {
      "kind": "Type",
      "name": "Option",
      "params": ["a"],
      "constructors": [
        { "name": "Some", "fields": ["a"] },
        { "name": "None", "fields": [] }
      ]
    },
    {
      "kind": "Fn",
      "name": "unwrap",
      "params": [{ "name": "opt", "type": "Option<Int>" }],
      "return_type": "Int",
      "effect_row": {},
      "body": {
        "kind": "Match",
        "scrutinee": { "kind": "Var", "name": "opt", "type": "Option<Int>" },
        "scrutinee_type": "Option<Int>",
        "arms": [
          {
            "pattern": {
              "kind": "Constructor",
              "name": "Some",
              "type_name": "Option",
              "fields": [{ "kind": "Bind", "name": "x", "type": "Int" }],
              "type": "Option<Int>"
            },
            "body": { "kind": "Var", "name": "x", "type": "Int" }
          },
          {
            "pattern": {
              "kind": "Constructor",
              "name": "None",
              "type_name": "Option",
              "fields": [],
              "type": "Option<Int>"
            },
            "body": {
              "kind": "Literal",
              "value": { "Int": "0" },
              "type": "Int"
            }
          }
        ],
        "result_type": "Int"
      }
    }
  ]
}
```

## Pretty-printing

`pretty_print(module)` renders an `IrModule` back to canonical Hirð source.
The printer is the inverse direction of lowering: it re-introduces the surface
forms lowering erased. Because lowering is not injective on syntax (operators,
`if`, and parentheses all collapse), the printed source is *canonical* rather
than a copy of any original — but it lowers back to the same IR.

Formatting:

- A `module <Name>` header, then one declaration per block, blocks separated by
  a blank line. Each declaration is rendered on a single logical line.
- Operators print infix using their canonical Unicode forms (`→`, `λ`, `∧`,
  `∨`), with parentheses inserted only where operator precedence or
  associativity would otherwise re-parse to a different tree (`(a + b) * c`,
  `(a == b) == c`).
- A lowered `if` prints as the `match` over `Bool` it became; a lowered handle
  prints as its body. These desugarings are not reversed — the IR is the
  canonical form.
- Function signatures print every parameter type and, where expressible, the
  return type. Record and unit (`()`) types have no annotation syntax, so a
  function returning one omits its (optional) return annotation and lets
  inference recover it. The empty effect row is elided (`! {}` is the surface
  default).
- Type-variable letters are renumbered to `a, b, c, …` in order of first
  appearance within each signature, so output does not depend on the
  unification-variable identities inference happened to assign.
- Extern parameter names are synthesised (`p0`, `p1`, …); the IR keeps only the
  signature type, and the names do not affect it.

## Round-trip property

For any well-typed module, lowering is stable through pretty-printing:

```
source → check → lower → pretty_print → check → lower
```

reproduces the first IR, **up to type-variable renaming**. This is a property
test (`tests/roundtrip.rs`), exercised over hand-written programs covering
every node kind and over proptest-generated well-typed programs. It is the
regression net for lowering and inference: it catches printer bugs (output that
fails to parse or re-check), lowering bugs (information lost on the way down),
and inference instability (re-checking the printed form yielding different
types).

Equality is taken modulo type-variable renaming because two sources of
variation are benign: inference assigns fresh unification-variable identities on
each run, and the printer may turn an inferred signature into a skolemised one
(annotating the return type moves a function onto the checker's rigid-skolem
path). Both genuine unification variables and skolem constants — which the lexer
guarantees are the only lowercase type names — are renumbered by first
appearance before comparing. Type declarations are compared verbatim: their
constructor field types are fixed by the declared parameter names, with no
inference freedom.

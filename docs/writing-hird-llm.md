# Writing Hirð — constraints and reference for LLM agents

This document is for LLM agents generating or editing Hirð code. It
lists the constraints the compiler enforces, the tooling available to
query the compiler instead of guessing, and the mistakes the compiler
most often catches in generated code.

Include [`phrasebook.md`](../phrasebook.md) in your context window: it
is the dense canonical-pattern reference (~3k tokens) and every
snippet in it reflects the implemented compiler. A complete, working
program exercising every major construct is
[`demo/agent_planner.hird`](../demo/agent_planner.hird).

## Hard constraints (compile errors, not style)

**Naming is enforced by the lexer.** `snake_case` for values,
functions, parameters, and bindings; `PascalCase` for types,
constructors, actors, supervisors, effects, and modules. A
lowercase-initial name may not contain an uppercase letter; an
uppercase-initial name may not contain `_`. There is no escape hatch.

**One canonical operator form.** `->`, `=>`, and `\` normalise to
`→`, `⇒`, and `λ` at lex time. Emit either; expect to read the
Unicode forms in existing source and compiler output.

**Effect rows are checked for equality.** A function's declared row
(`! {…}`) must equal exactly what its body performs — not a superset,
not a subset. An omitted annotation means `! {}` (pure), so any
effectful function *must* be annotated. Effect polymorphism is
expressed with a row tail (`! {r}` or `! {Log, r}`), never by
subsumption.

**Effect heads need declarations.** Only `Install`, `Supervise`, and
`Stand` are built in. Before a row can name `Tool<…>`, `Send<…>`, `Await<…>`,
`Spawn<…>`, or `Exn<…>`, the program must declare them:
`effect Tool<t>`, `effect Send<t>`, etc.

**Expression-oriented, no blocks.** Every body is one bare expression
after `=`; sequence with `let x = e in …`. Braces in expression
position are always record literals. `if` always has `then` and
`else`. `match` arms are comma-separated and must be exhaustive.

**No ambient state.** There is no global `now()`, `random()`,
`print()`, or logging. Non-determinism and I/O enter a function only
through capability parameters (typed opaque handles) and tool calls,
and both show up in the effect row. Opaque types cannot be
constructed or destructured outside their declaring module — do not
try to work around a capability by pattern-matching it open.

**Tools are the only I/O boundary.** `tool Name : {args} → result`
creates the `Tool<Name>` effect and a callable `name` function. Tool
signatures must be wire-representable (no function types, no opaque
capabilities). Implementations come from `handle` blocks (lexical,
discharges the effect) or `install` blocks (runtime registry, for
spawned/supervised actors; handlers must be pure; does not discharge
the body's row). Handler maps never cross a process boundary: a
`handle` block wrapped around `spawn`/`supervise` does nothing for
the spawned actor — use `install`.

**Entry point.** `hird run` requires exactly one `fn main() → ()`
with no parameters and no residual `Tool<…>` in its row. `Install`,
`Supervise`, `Stand`, `Send`, `Await` may remain. A program halts when
`main` returns, supervision trees included; end `main` with `stand()`
(effect `Stand`) to keep it up until SIGTERM or Ctrl-C, which shuts the
trees down and syncs the audit stream first.

## Querying the compiler

Do not infer types, effect rows, or actor protocols from reading
source when you can ask the compiler.

### CLI (available now)

```sh
hird check <file-or-dir>                  # full type/effect check, coded diagnostics
hird emit-ast <file> --json               # typed IR of every definition
hird emit-effect-graph <file> --json      # actors, mailboxes, handler rows, supervisors, tools
hird build <file>                         # + emit and compile Erlang
hird run <file>                           # + execute; audit JSON lines on stdout
```

The tight loop is: generate → `hird check` → read the diagnostic
codes → fix → repeat. Diagnostics are structured (code, message,
span) and the codes are stable; the table below maps the common ones.

### MCP server (`hird-mcp`)

The MCP server exposes the same compiler pipeline as structured tools
for agent frameworks. The `hird-mcp` binary speaks MCP over stdio
(newline-delimited JSON-RPC); point your MCP client at it with no
arguments. Errors (missing file, undefined name, parse or type errors)
come back as structured `isError` results carrying diagnostics.

| Tool | Returns |
|---|---|
| `infer_type(file, expr_location)` | Inferred type and effect row of an expression. |
| `lookup_definition(file, name)` | Source location, type, doc, kind of a definition. |
| `explain_effect_row(file, fn_name)` | A function's row with each effect explained. |
| `render_ir_fragment(file, name)` | IR JSON for one definition. |
| `explain_actor_protocol(file, actor_name)` | Message constructors, state type, handler signatures, effect summary. |
| `emit_actor_effect_graph(file, actor_name)` | Actor/effect graph rooted at the actor: supervisors, transitive tool effects. |
| `get_context_for_symbol(file, name, budget)` | Token-budget-aware symbol summary (type, row, callers, callees). |
| `get_context_budget(file)` | Approximate token cost of the project's types/effects/actors/tools. |

Prefer `explain_actor_protocol` / `emit_actor_effect_graph` over
reading actor source: effect rows are per-process and local by
design, so "what does this actor transitively do" is a tooling query,
not something visible in any one signature.

## Common generated-code mistakes and how the compiler catches them

| Mistake | Caught by |
|---|---|
| Effectful body with `! {}` or no annotation | C0030 — declared row ≠ inferred row, anchored at the introducing call |
| Declaring an effect the body never performs | C0030 (equality cuts both ways) |
| Using `Tool`/`Send`/`Await`/`Spawn`/`Exn` in a row without `effect` declarations | C0027 unknown effect |
| Non-exhaustive `match` over a sum type | C0015, listing the missing constructors |
| Matching `Some`/`None` without declaring `type Option<a> = Some(a) \| None` (built-in `Option`/`List` carry no constructors) | C0007 unknown constructor |
| Constructing or destructuring an opaque capability outside its module | C0022 / C0021 |
| `Tool<X>` arm where `X` is not a declared tool | C0033 |
| Handler whose type does not match the tool's signature | C0034 (non-function handler: C0031) |
| Effectful handler in an `install` block | C0051 — installed handlers must be pure |
| `stand()` inside an actor's `init` or handler | C0054 — it would park the actor's process; stand from `main` |
| Function or capability types in a tool signature | C0032 — not wire-representable |
| `f { a: 1 }` instead of `f({ a: 1 })` | parse error — `{` never starts an application argument |
| Chained comparisons `a == b == c` | P0005 — relational operators do not associate |
| Missing comma between `match`/`handle` arms | P0001 |
| camelCase / SCREAMING_CASE identifiers | lexer rejection, surfacing as P0001/P0002 parse errors |
| Statement-block bodies `fn f() = { let x = 1; x }` | parse/check errors — `{…}` is a record; use `let … in` |

Two semantic traps that type-check but behave unexpectedly:

- `install` does not discharge the body's `Tool<…>` effects; only
  `handle` does. A `main` that calls tools directly needs `handle`.
- `request` blocks with a fixed 5000ms timeout and a timeout *crashes
  the caller* (no `Exn` in the row); dropped or double `reply` also
  surfaces as a timeout. Crash handling belongs to supervisors, never
  to the caller.

## Checklist before emitting a program

1. Every effect head used in a row is declared (`effect Tool<t>`, …).
2. Every function's row equals its body's effects exactly.
3. Every `match` covers every constructor (or has `_`).
4. Actor handler rows are per-handler, and the actor's trailing
   summary equals their union; handlers cover the whole message type.
5. Tool calls resolve: `handle` in-process, `install` for actors.
6. `main` is `fn main() → ()`, no params, no residual `Tool<…>`.
7. Run `hird check` and iterate on the codes — it is cheaper than
   being clever.

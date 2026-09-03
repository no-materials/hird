# Hirð Phrasebook

> Dense reference for inclusion in LLM context windows.
> Each section shows the canonical pattern, not a tutorial explanation.

---

## Naming Rules (compiler-enforced)

```
snake_case    — values, functions, parameters, local bindings
PascalCase    — types, actors, supervisors, effects, modules, constructors
a, b, r       — type and row variables (any snake_case name in type
                position; single letters by convention)
```

The lexer enforces canonical case: a lowercase-initial name may not
contain an uppercase letter, an uppercase-initial name may not contain
`_` (so `SCREAMING_CASE` is rejected). Violations are compile errors
(surfacing as parse errors), not warnings.

---

## Unicode Operator Forms

Both forms lex identically. The canonical form is the Unicode version.

| ASCII | Unicode | Token   |
|-------|---------|---------|
| `->`  | `→`     | Arrow   |
| `=>`  | `⇒`    | FatArrow |
| `\`   | `λ`     | Lambda  |
| `&&`  | `∧`     | AmpAmp  |
| `\|\|`  | `∨`     | PipePipe |

Logical operators `&&` (`∧`) and `||` (`∨`) are left-associative and bind
looser than comparisons: `a == b && c == d` means `(a == b) && (c == d)`.

Relational operators (`==` `!=` `<` `>` `<=` `>=`) do not chain — they are
one non-associative precedence tier. Write `(a == b) == c`, never
`a == b == c`.

---

## Function Declarations

```
fn add(x: Int, y: Int) → Int ! {} = x + y

fn read_config(path: Path) → Config ! {Tool<ReadFile>, Exn<ParseError>} = ...

fn map(f: a → b ! {r}, xs: List<a>) → List<b> ! {r} = ...
```

- `! {}` is the empty effect row (pure). Elided in display.
- `! {r}` is an open row variable (effect-polymorphic).

```
fn tick(clock: Clock, pid: Pid<Msg>) → () ! {Tool<Log>, Schedule<Msg>} =
  log({ message: "tick" });
  schedule(clock, pid, Tick, 1000)
```

- `a; b` sequences: `a` runs for its effects and must be `()` (C0058
  otherwise), then `b` is the result. Sugar for `let _ = a in b`, which
  accepts any type. Right-associative and looser than every operator, so a
  `;` inside a `let` body, an `if` branch, or a match arm belongs to that
  body. Bodies stay single expressions — there are no statement blocks.

---

## Type Declarations (ADTs)

```
type Result<a, e> = Ok(a) | Err(e)

type List<a> = Cons(a, List<a>) | Nil

type PlannerMsg =
  | PlanRepo(Path)
  | GetStatus(ReplyTo<PlannerStatus>)
  | Shutdown
```

Constructors are typed functions: `Ok : ∀a e. a → Result<a, e>`. `Option<a> =
Some(a) | None` is predeclared (like `Bool` and `Next`); a user declaration
shadows it.

---

## Type Aliases

```
type alias LogArgs = { level: String, message: String }
type alias Handler<a> = a → () ! {Tool<Log>}

tool Log : LogArgs → ()
fn demo_log(args: LogArgs) → () = ()
```

An alias is a name for a type, not a new type: every use expands to the
right-hand side, so `LogArgs` and `{ level: String, message: String }` are
the same type everywhere (unification, the wire check, the audit format,
the IR). Any type expression may be aliased; a parametric alias is applied
at its declared arity. An alias cannot refer to itself, directly or through
other aliases (C0059) — recursion is for ADTs — and cannot be `opaque`
(P0007): identity stays with `type`. `pub type alias` exports it; an
importer sees the expansion.

---

## Records

```
type alias HeartState = { clock: Clock, period: Int, beats: Int }

fn beat(st: HeartState) → HeartState = { beats: st.beats + 1, ..st }
```

Records are structural and closed: `{ x: 1, y: 2 }` has type `{ x: Int,
y: Int }` and nothing else. Fields are read with `st.beats`. A literal
ending in `..base` is an update: the listed fields come from the literal,
every other field from `base`, and the result has the type of `base` — so a
field the base lacks is an error (C0010), the base must be a known record
(C0009), and `{ ..base }` alone is rejected rather than treated as a copy
(C0060). One base, written last (P0008). It lowers to an Erlang map update.

---

## Pattern Matching

```
match msg {
  PlanRepo(path) → handle_plan(path, state),
  GetStatus(reply_to) → handle_status(reply_to, state),
  Shutdown → handle_shutdown(state),
}
```

- Exhaustiveness is required. Missing constructors are compile errors.
- Patterns: constructors, variables, wildcards (`_`), literals, tuples, nested.

```
let Config(clock, period) = config in …
let (a, b) = pair in …
let _ = compute() in …
```

- `let` binds a pattern. A plain name is generalised (let-polymorphism); a
  destructuring pattern binds its variables monomorphically, like a match
  arm, and must be irrefutable: `let Some(x) = o in …` is a compile error
  (C0057) — use `match`.

---

## Effect Declarations

```
effect Log
effect Audit<t>
effect EtsRead<t>
```

Parametric effects reference specific capabilities or message types.

Declarations are for user heads only. The checker-known heads are built in
and need none: bare `Install`, `Supervise`, `Stand`, `Clock`, and
parametric `Tool`, `Send`, `Await`, `Spawn`, `Schedule`, `Exn`. Declaring
one of those is redundant and warns (C0056).

---

## Tool Declarations

```
tool ReadRepo : { path: Path } → RepoState
tool CreateTicket : { title: String, body: String } → TicketId
tool Log : { level: String, message: String } → ()
tool LLMCall<t> : { prompt: Prompt, schema: Schema<t> } → t ! {Exn<ParseError>}
```

Each tool declaration creates:
1. An effect (`Tool<ReadRepo>` — one shared `Tool` effect, applied to the
   tool's marker type).
2. A callable function (`read_repo`, `create_ticket`, `llm_call`).
3. A compiler-generated invocation record type (`ReadRepoInvocation`).

A trailing `! {…}` row unions into the function's row. LLM calls are
schema-typed: the schema argument fixes the result type, and a
non-conforming response raises `Exn<ParseError>`.

```
fn triage(p: Prompt, s: Schema<Ticket>) → Ticket
  ! {Tool<LLMCall>, Exn<ParseError>} = llm_call({ prompt: p, schema: s })
```

---

## Actor Declarations

```
actor Planner {
  state: PlannerState,

  message: PlannerMsg =
    | PlanRepo(Path)
    | GetStatus(ReplyTo<PlannerStatus>)
    | Shutdown,

  init: fn(config: PlannerConfig) ! {Tool<Log>} = initial_state(config),

  handle PlanRepo(path), st
    ! {Tool<ReadRepo>, Tool<CreateTicket>, Tool<Log>} = plan_repo(path, st),

  handle GetStatus(reply_to), st
    ! {Send<PlannerStatus>} = reply_status(reply_to, st),

  handle Shutdown, _ ! {} = Stop,
} ! {Tool<ReadRepo>, Tool<CreateTicket>, Tool<Log>, Send<PlannerStatus>}
```

- Handler and `init` bodies follow the uniform bare-body rule (`= e`);
  braces never wrap a body.
- A handler binds the message payload pattern, then the current state as a
  trailing comma-separated pattern (`_` if unused). The state pattern's type
  is the declared `state` type; the binder name is the author's choice.
- A handler body produces a `Next<State>` outcome, never a bare state:
  `Continue(next)` keeps the actor running, `Stop` stops it deliberately.
  Neither a handler nor `init` writes a return type — the outcome type is
  fixed by `state:`, and a `→ …` in either position is a parse error
  (P0006).
  `Next<a> = Continue(a) | Stop` is predeclared (constructors and all, like
  `Bool`), so helpers may build and return outcomes. A deliberate stop is
  not a crash: a `transient` child stays stopped, a `permanent` one is
  restarted. `init` still returns the bare state.
- State is encapsulated — inaccessible outside handlers.
- Handlers must be exhaustive over the message type.
- Per-actor effect summary is declared and checked.

---

## Supervisor Declarations

```
supervisor PlannerSup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: planner, actor: Planner, start_args: default_config(), restart: permanent },
  ]
}
```

- Body fields are a closed set — `strategy`, `intensity`, `period`, `children`
  — each required, each written once. `intensity` and `period` are positive
  integers, with no defaults.
- Each child names a declared `actor`, a unique lowercase `id`, a pure
  `start_args` checked against the actor's single init parameter, and a
  `restart` of `permanent`, `temporary`, or `transient`.
- The supervisor's effect row is derived (the union of its children's per-actor
  summaries), never declared — there is no trailing `! { … }`.
- `strategy` is `one_for_one`, `one_for_all`, or `rest_for_one`, with OTP's
  restart semantics.

---

## Typed References

```
Pid<PlannerMsg>         — typed process reference
ReplyTo<PlannerStatus>  — typed reply channel

spawn(Planner, config) → Pid<PlannerMsg> ! {Spawn<PlannerMsg>}
send(pid, PlanRepo(path)) → () ! {Send<PlannerMsg>}
request(pid, GetStatus) → PlannerStatus ! {Send<PlannerMsg>, Await<PlannerStatus>}
request(pid, GetStatus, 60000) → PlannerStatus ! {Send<PlannerMsg>, Await<PlannerStatus>}
reply(reply_to, status) → () ! {Send<PlannerStatus>}

supervise(PlannerSup) → () ! {Supervise}
child(PlannerSup, planner) → Pid<PlannerMsg> ! {}
stand() → () ! {Stand}

clock() → Clock ! {Clock}
schedule(clock, pid, Tick, 1000) → () ! {Schedule<PlannerMsg>}
self() → Pid<PlannerMsg> ! {}          — inside Planner's init/handlers only
```

- `Pid<t>` and `ReplyTo<t>` are built-in type constructors (like `List<t>`);
  `ReplyTo<t>` is a distinct type, not an alias of `Pid<t>`.
- `spawn` is a keyword form: its first argument is an actor name, resolved in
  the actor namespace. Actor names are not first-class values.
- `send`, `request`, and `reply` are keyword forms as well. `reply` is the
  only operation on `ReplyTo<t>` — it is not an overload of `send`.
- `request` blocks for its timeout — an optional trailing `Int` of
  milliseconds, 5000 when omitted; a timeout exits the caller (no `Exn` in
  the row — crash handling belongs to supervision). The timeout is not an
  effect: the row is the same with or without it.
- `Spawn<t>`, `Send<t>`, `Await<t>`, `Schedule<t>` are built-in effect heads
  (no declaration needed) whose semantics the checker knows, like `Tool<t>`.
- `supervise` starts a declared supervisor's tree: the name resolves in the
  supervisor namespace (supervisor names are not values either). One running
  instance per declaration — a second `supervise` of the same name crashes.
  Its bare `Supervise` effect is checker-known (like `Install`): no
  declaration needed.
- `child` is typed child lookup: the id must be one of the supervisor's
  declared children, and the result is `Pid<Msg>` for the child actor's
  message type. Effect-free; a missing or restarting child crashes
  (`{no_child, id}`) — tree health is supervision's concern, never a
  caller-recoverable error.
- `stand` keeps the program up: it blocks the caller until the program is
  asked to stop (Ctrl-C under `hird run` on any platform; SIGTERM to the
  node where the platform has it), then shuts down every
  supervisor the caller started and returns, so `main` finishes and the
  audit stream is synced before the halt. Without it a program halts when
  `main` returns, trees included. Its bare `Stand` effect is checker-known
  (like `Supervise`). Not allowed inside an actor's `init` or handlers
  (C0054) — it would park the actor's process.
- Time is a capability. `Clock` is a built-in opaque type; `clock()` is the
  one way to obtain one and carries the checker-known bare effect `Clock`,
  so a function or actor that reaches for real time — rather than being
  handed a clock — says so in its row. `clock` is contextual: only
  `clock()` is the form, so `clock: Clock` is a fine parameter name.
- `schedule(clock, pid, msg, delay_ms)` delivers `msg` to `pid` after
  `delay_ms` milliseconds (an `Int`; no duration type) through the clock;
  its effect is `Schedule<Msg>`, a head of its own, so the row tells a
  self-driving actor from one that merely sends. The timer is not returned
  and cannot be cancelled; a message aimed at an exited pid is dropped, so a
  restarted actor schedules its first tick again in `init`.
- `self()` is the enclosing actor's own `Pid<Msg>`, effect-free; only
  inside an actor's `init` or handler body (C0055 elsewhere). Schedule to
  `self()` for a periodic tick.
- A supervisor child's `start_args` must be pure except for acquiring the
  clock (`start_args: HeartConfig(clock(), 1000)`) — the child spec is where
  a supervised actor is handed its capabilities — and the supervisor's
  derived row records the `Clock`.

---

## Handle Blocks (DI-style)

```
handle {
  Tool<ReadRepo> → mock_read_repo,
  Tool<CreateTicket> → mock_create_ticket,
  Tool<Log> → unit_log,
} in planner_main(config)
```

Handlers replace effect implementations within the block scope.
Use for: mocking, dry-runs, log redirection, audit interception.
Declare logging as a tool (`Tool<Log>`): bare effects have no
compiler-known operation in v0.1, so a bare-effect arm type-checks and
threads but is never invoked by emitted code.

---

## Install Blocks (registry defaults)

```
install {
  Tool<ReadRepo> → demo_read_repo,
  Tool<CreateTicket> → demo_create_ticket,
} in run_demo(config)
```

Installs default handlers in the runtime registry for the dynamic extent of
the body, then restores the previous entries (crash included). Handler maps
never cross `spawn`, so this is how spawned actors' tool calls resolve.
Use for: supplying deployment/demo handlers, test-harness mocks for actors.

- Arms are `handle`'s: same grammar, same checking (including tool operation
  signatures).
- Installed handlers must be pure — their effect row closed and empty; they
  run later, in arbitrary processes.
- The expression's row is the body's row plus `Install`, a checker-known bare
  effect head (pre-declared, like `Supervise` — no user declaration needed).
- Unlike `handle`, `install` does not discharge the body's `Tool<…>` effects
  from the row — the handlers run later, in whatever process dispatches them.
- Entries are visible to *all* processes while the body runs; the restore is
  best-effort under concurrency.

---

## Capability Discipline

No ambient state. Every non-deterministic or external operation requires a
capability parameter:

```
fn lookup(t: Table<UserId, User, Read>, key: UserId) → Option<User>
  ! {EtsRead<t>}

fn now(clock: Clock) → Timestamp ! {ClockRead<clock>}
fn rand(rng: Random) → Float ! {RandomRead<rng>}
fn info(log: Log, msg: String) → () ! {LogWrite<log>}
```

`Clock` is the one built-in capability today: acquired by `clock()`
(effect `Clock`), consumed by `schedule` (effect `Schedule<Msg>`). The
others are the shape the discipline takes for user-declared capabilities.

---

## Errors vs Crashes

- **Domain errors**: `Exn<ParseError>`, `Exn<HttpError>` — values in effect rows.
  Handled with pattern matching or effect handlers. Do not kill the process.
- **Crashes**: `crash!("msg")` (alias `panic!`) — divergent, reaches
  supervisor. For truly unrecoverable situations. Cannot be caught in normal
  code. Typed `∀a. (String) → a`, so it fits any result position; not an
  effect, so it never appears in the row.

---

## Common Pitfalls

- **Non-exhaustive match** (C0015): cover every constructor or add a `_`
  arm. Arms are comma-separated; omitting the comma is a parse error.
- **Effect rows check for equality** (C0030), not subset: an omitted row is
  `! {}`, so an effectful body with no annotation fails — and so does
  declaring an effect the body never performs. Effect-polymorphic code
  names a row tail (`! {Log, r}`) instead of relying on subsumption.
- **Opaque types outside their module**: constructing (C0022) or
  destructuring (C0021) an opaque type outside its declaring module is a
  compile error — that is the capability discipline doing its job.
- **Undeclared effect heads** (C0027): only user heads (`Audit<t>`,
  `EtsRead<t>`, …) need an `effect` declaration before a row names them.
  `Tool`, `Send`, `Await`, `Spawn`, `Schedule`, `Exn`, `Install`,
  `Supervise`, `Stand`, and `Clock` are built in; redeclaring one warns
  (C0056).
- **Milliseconds are bare `Int`s**: `request(p, Get, 60000)` and
  `schedule(c, p, Tick, 1000)` take millisecond counts; there is no
  duration type or literal.
- **Tool arms**: `Tool<X>` requires `X` to be a declared tool (C0033); the
  handler must match the tool's operation signature (C0034); `install`
  handlers must be pure — closed empty row (C0051).
- **Tool signatures must be wire-representable** (C0032): no function
  types, no opaque capability types in args or result.
- **Record arguments need parens**: `f({ title: t })`, never `f { title: t }`
  (a `{` never starts an application argument).
- **Relational operators do not chain**: `a == b == c` is a parse error
  (P0005); write `(a == b) == c`.
- **`;` needs a `()` left operand** (C0058): `f(x); g()` requires `f(x)` to
  return `()`. To drop a value on purpose, write `let _ = f(x) in g()`.
- **`Cons`/`Nil` are not predefined**: `List` is a built-in type name with
  no constructors until list literals and patterns exist. `Option` (with
  `Some`/`None`), `Next` (with `Continue`/`Stop`), and `Bool` are
  predeclared, so they need no declaration.
- **Handlers return `Next<State>`, not `State`**: `= st` in a handler is a
  type error — write `= Continue(st)`; stop with `= Stop`.
- **Handler maps never cross `spawn`/`supervise`**: a `handle` block around
  a spawn does nothing for the spawned actor's tool calls — use `install`.

---

## Modules

```
module Planner

use Ets.{Table, lookup}
use Log as L

pub fn plan(config: PlannerConfig) → Plan ! {Tool<ReadRepo>, Tool<Log>} = ...
```

- `pub` for exports; `pub opaque type` exports the name but keeps
  constructors module-private. Unprefixed is module-private.
- `use Mod` and `use Mod as M` bind a qualifier for `Mod.member` / `M.member`.
  `use Mod.{a, b}` binds `a` and `b` unqualified — and only that: it does not
  also bind the `Mod` qualifier.
- One module per file; the module name is derived from the file path, and a
  `module` declaration must match it.

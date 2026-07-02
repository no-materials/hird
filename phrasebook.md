# Hirð Phrasebook

> Dense reference for inclusion in LLM context windows.
> Each section shows the canonical pattern, not a tutorial explanation.
> Target: under 4000 tokens when finalized.

---

## Naming Rules (compiler-enforced)

```
snake_case    — values, functions, parameters, local bindings
PascalCase    — types, actors, supervisors, effects, modules, constructors
a, b, r       — type variables (single lowercase letter)
SCREAMING     — (not used; reserved)
```

Violations are compile errors, not warnings.

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

---

## Type Declarations (ADTs)

```
type Option<a> = Some(a) | None

type List<a> = Cons(a, List<a>) | Nil

type PlannerMsg =
  | PlanRepo(Path)
  | GetStatus(ReplyTo<PlannerStatus>)
  | Shutdown
```

Constructors are typed functions: `Some : ∀a. a → Option<a>`.

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

---

## Effect Declarations

```
effect Log
effect Tool<t>
effect EtsRead<t>
effect Send<t>
effect Await<t>
effect Spawn<t>
```

Parametric effects reference specific capabilities or message types.

---

## Tool Declarations

```
tool ReadRepo : { path: Path } → RepoState
tool CreateTicket : { title: String, body: String } → TicketId
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

  init: fn(config: PlannerConfig) → PlannerState ! {Log},

  handle PlanRepo(path) → PlannerState
    ! {Tool<ReadRepo>, Tool<CreateTicket>, Log} { ... },

  handle GetStatus(reply_to) → PlannerState
    ! {Send<PlannerStatus>} { ... },

  handle Shutdown → PlannerState ! {} { ... },
} ! {Tool<ReadRepo>, Tool<CreateTicket>, Log, Send<PlannerStatus>}
```

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

---

## Typed References

```
Pid<PlannerMsg>         — typed process reference
ReplyTo<PlannerStatus>  — typed reply channel

spawn(Planner, config) → Pid<PlannerMsg> ! {Spawn<PlannerMsg>}
send(pid, PlanRepo(path)) → () ! {Send<PlannerMsg>}
request(pid, GetStatus) → PlannerStatus ! {Send<PlannerMsg>, Await<PlannerStatus>}
```

---

## Handle Blocks (DI-style)

```
handle {
  Tool<ReadRepo> → mock_read_repo,
  Tool<CreateTicket> → mock_create_ticket,
  Log → capturing_log,
} in planner_main(config)
```

Handlers replace effect implementations within the block scope.
Use for: mocking, dry-runs, log redirection, audit interception.

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

---

## Errors vs Crashes

- **Domain errors**: `Exn<ParseError>`, `Exn<HttpError>` — values in effect rows.
  Handled with pattern matching or effect handlers. Do not kill the process.
- **Crashes**: `crash!("msg")` — divergent, reaches supervisor. For truly
  unrecoverable situations. Cannot be caught in normal code.

---

## Modules

```
module Planner

use Ets.{Table, lookup}
use Log as L

pub fn plan(config: PlannerConfig) → Plan ! {Tool<ReadRepo>, Log} = ...
```

- `pub` for exports. Unprefixed is module-private.
- Qualified names: `Ets.lookup`.
- One module per file.

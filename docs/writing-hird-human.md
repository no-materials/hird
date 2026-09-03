# Writing Hirð — a guide for human developers

Hirð is a typed language for long-running agent systems on BEAM. Its
one organizing idea: every side effect a function performs — calling a
tool, sending a message, spawning a process — is visible in its type,
and every external invocation is recorded on an audit stream. This
guide gets you from nothing to a supervised, message-passing program.

For a dense syntax reference, see [`phrasebook.md`](../phrasebook.md).
If you are an LLM agent rather than a human, read
[`writing-hird-llm.md`](writing-hird-llm.md) instead.

## Getting started

You need Rust (1.97+) to build the compiler and Erlang/OTP on `PATH`
to build and run programs (`hird check` alone works without Erlang).

```sh
git clone <repo> && cd hird
cargo build -p hird-cli
alias hird='cargo run -q -p hird-cli --'
```

### Hello, world

Hirð has no ambient `print`. Anything a program tells the outside
world goes through a *tool* — a declared, typed, audited external
operation. The audit stream prints one JSON line per tool call to
stdout, so the smallest observable program is a tool call:

```
module Hello

tool Say : { message: String } → ()

fn quiet_say(args: { message: String }) → () = ()

fn main() → () ! {} =
  handle {
    Tool<Say> → quiet_say,
  } in say({ message: "hello, world" })
```

Save it as `hello.hird`, then:

```sh
hird run hello.hird
```

```json
{"schema_version":1,"tool":"Say","args":{"message":"hello, world"},"result":{"ok":null},"timestamp":"…","caller":"Hello.main"}
```

Three things happened. Declaring `tool Say` created a callable
function `say` whose effect row carries `Tool<Say>`. The `handle`
block supplied an implementation and discharged the effect, so `main`
is honestly `! {}`. And the call was recorded on the audit stream —
unconditionally; mocked and real tool calls audit identically.

`hird build hello.hird` runs the same pipeline but stops after
compiling the generated Erlang into `_build/hird/`; the emitted
`.erl` files are human-readable — go look at them.

## Language tour

### Values, functions, expressions

Hirð is expression-oriented: every body is one bare expression. There
are no statement blocks: `a; b` runs `a` for its effects (it must return
`()`) and then evaluates to `b`, and `let x = e in …` names a value for
what follows. A `let` binds
a pattern, so a single-constructor value or a tuple destructures in
place (`let Config(clock, period) = config in …`); a pattern that could
fail to match (`Some(x)`) is rejected — that is what `match` is for.

```
fn clamp(x: Int, lo: Int, hi: Int) → Int =
  if x < lo then lo else if x > hi then hi else x

fn describe(x: Int) → String =
  let bounded = clamp(x, 0, 10) in
  if bounded == 10 then "big" else "small"
```

Names are compiler-enforced: `snake_case` for values and functions,
`PascalCase` for types, constructors, actors, supervisors, effects,
and modules. The lexer rejects anything else.

ASCII operator spellings normalise to Unicode at lex time: `->`
becomes `→`, `=>` becomes `⇒`, `\` becomes `λ`. Both forms are legal
input; the canonical form is the Unicode one.

### Types

Algebraic data types, structural records, tuples, and lambdas:

```
type Shape = Square(Int) | Rect(Int, Int)

fn area(s: Shape) → Int =
  match s {
    Square(side) → side * side,
    Rect(w, h) → w * h,
  }

fn origin() → { x: Int, y: Int } = { x: 0, y: 0 }

fn twice(f: a → a, x: a) → a = f(f(x))
```

`match` must be exhaustive — a missing constructor is a compile
error, not a warning. Records are structural (`{ x: Int, y: Int }`)
and accessed with `point.x`. Generics use lowercase type variables
(`a`, `b`) with no declaration needed.

A `type alias` names a shape without creating a new type:

```
type alias Point = { x: Int, y: Int }

fn origin() → Point = { x: 0, y: 0 }
```

`Point` and `{ x: Int, y: Int }` are the same type everywhere; the
alias is expanded as the compiler reads it. Use one to name a record,
tuple, or function type you would otherwise spell out repeatedly — a
tool's argument record, say. When a type needs an identity of its own
(a capability, a message), declare an ADT with `type` instead; an alias
cannot be recursive or `opaque`.

A record literal may end in `..base` to rebuild an existing record with
some fields changed:

```
fn move_x(p: Point, dx: Int) → Point = { x: p.x + dx, ..p }
```

The listed fields come from the literal and every other field from `p`;
the result has `p`'s type, so an update can neither add nor remove
fields. This is how an actor handler rebuilds record-shaped state:
`Continue({ beats: st.beats + 1, ..st })`.

`Option<a> = Some(a) | None` is predeclared, like `Bool`. `List` is a
built-in type *name* with no constructors in v0.1; there are no list
literals or patterns yet.

### Modules

One module per file; the module name derives from the file path.
`pub` exports, `pub opaque type` exports a type's name while keeping
its constructors private — this is how capability types are made
unforgeable.

```
use Ets                  // qualified access: Ets.lookup
use Log as L             // qualified access: L.info
use Repo.{RepoState}     // binds RepoState unqualified (and only that)
```

## The effect system

Every function type ends in an effect row: `! {}` is pure, and an
omitted row means `! {}`. The row lists what the function's own
process does — call tools, send messages, spawn processes:

```
fn file_tickets(backlog: Backlog) → Int ! {Tool<CreateTicket>, Tool<Log>} =
  ...
```

The row is checked for **equality** against what the body actually
performs. Under-declaring is an error; so is over-declaring. To write
functions generic over their argument's effects, name a row variable:

```
fn map(f: a → b ! {r}, xs: List<a>) → List<b> ! {r} = ...
```

User effect heads are declared like types (`effect Log`,
`effect Audit<t>`). The heads the compiler knows — `Tool`, `Send`, `Await`,
`Spawn`, `Schedule`, `Exn`, `Install`, `Supervise`, `Stand`, `Clock` — are
built in and need no declaration.

### Tools

A `tool` declaration is the boundary to the outside world:

```
tool ReadRepo : { path: Path } → RepoState
tool LLMCall<t> : { prompt: Prompt, schema: Schema<t> } → t ! {Exn<ParseError>}
```

Each declaration yields an effect (`Tool<ReadRepo>`), a callable
function (`read_repo`), and a compiler-derived invocation record that
the audit stream serialises. Tool signatures must be
wire-representable: no function types, no opaque capabilities.

Tools have no "real" implementation in the language — implementations
are always supplied by handlers, which makes mocking, dry-running,
and auditing the default rather than an afterthought.

### Handlers: `handle` and `install`

`handle { … } in body` supplies implementations lexically and
discharges the handled effects from the row — use it for mocks,
dry-runs, and log redirection within one process:

```
handle {
  Tool<ReadRepo> → mock_read_repo,
} in planner_main(config)
```

Handler maps never cross a process boundary. Spawned and supervised
actors resolve tool calls through the runtime *registry* instead,
populated by `install { … } in body`: entries are visible to all
processes for the body's dynamic extent, then restored. Installed
handlers must be pure. `install` adds `Install` to the row and does
not discharge the body's tool effects — the handlers run later, in
whatever process calls the tool.

## Actors and supervision

An actor is a typed OTP `gen_server`: a state type, a message sum
type, an `init`, and one handler per message constructor
(exhaustiveness is checked). Each handler declares its own effect
row, and the actor's trailing summary must equal their union:

```
actor Planner {
  state: PlannerState,

  message: PlannerMsg =
    | PlanRepo(Path)
    | GetStatus(ReplyTo<PlannerStatus>)
    | Shutdown,

  init: fn(config: PlannerConfig) ! {} = PlannerState(0, 0),

  handle PlanRepo(path), PlannerState(repos, tickets)
    ! {Tool<ReadRepo>, Tool<CreateTicket>, Tool<Log>} = ...,

  handle GetStatus(reply_to), st
    ! {Send<PlannerStatus>} = ...,

  handle Shutdown, st ! {} = st,
} ! {Tool<ReadRepo>, Tool<CreateTicket>, Tool<Log>, Send<PlannerStatus>}
```

Interaction goes through keyword forms, each with its effect:

```
spawn(Planner, config)          → Pid<PlannerMsg>  ! {Spawn<PlannerMsg>}
send(pid, PlanRepo(path))       → ()               ! {Send<PlannerMsg>}
request(pid, GetStatus)         → PlannerStatus    ! {Send<PlannerMsg>, Await<PlannerStatus>}
reply(reply_to, status)         → ()               ! {Send<PlannerStatus>}
```

`request` sends a constructor carrying a `ReplyTo<T>` and blocks for
the answer; an optional third argument is the timeout in milliseconds
(`request(pid, GetStatus, 60000)`; 5000 when omitted), and a timeout
crashes the caller — see the error model below. The timeout is not an
effect, so the row is the same either way. Inside the handler,
`reply(reply_to, value)` answers it.

Time is a capability, not an ambient service. `Clock` is a built-in
opaque type; `clock()` is the one way to get one and carries the
built-in effect `Clock`, so whoever reaches for real time says so in
their row. `schedule(clock, pid, msg, delay_ms)` delivers a message
after a delay (milliseconds, a plain `Int`) with the effect
`Schedule<Msg>`, and `self()` — inside an actor's `init` or handlers —
is the actor's own `Pid<Msg>`. Together they make a periodic actor:

```
actor Heart {
  state: HeartState,
  message: HeartMsg = | Beat,
  init: fn(config: HeartConfig) ! {Schedule<HeartMsg>} =
    let HeartConfig(clock, period) = config in
    schedule(clock, self(), Beat, period);
    HeartState(clock, period, 0),
  handle Beat, HeartState(clock, period, beats)
    ! {Tool<Log>, Schedule<HeartMsg>} =
    log({ message: "beat" });
    schedule(clock, self(), Beat, period);
    Continue(HeartState(clock, period, beats + 1)),
} ! {Tool<Log>, Schedule<HeartMsg>}
```

The heart is *handed* its clock: a supervisor child's `start_args` may
acquire one (`start_args: HeartConfig(clock(), 1000)`) — the only effect
a start argument is allowed — and the supervisor's derived row records
the `Clock`. A scheduled message cannot be cancelled, and one aimed at a
process that has since exited is dropped; that is why the first beat is
scheduled in `init`, so a restarted heart starts beating again by
itself. [`demo/heartbeat.hird`](../demo/heartbeat.hird) is the full
standing program.

Supervisors declare a restart strategy over actor children; their
effect row is derived from the children, never written:

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

`supervise(PlannerSup)` starts the tree (effect `Supervise`);
`child(PlannerSup, planner)` looks up the running child as a typed
`Pid<PlannerMsg>`. A program halts when `main` returns, tree included —
end `main` with `stand()` (effect `Stand`) to keep it up until Ctrl-C
(or SIGTERM, where the platform has it), which shuts the trees down and
syncs the audit stream before
the halt. The full worked example is
[`demo/agent_planner.hird`](../demo/agent_planner.hird) — a
supervised planner driven end to end by `hird run`.

## The error model: errors vs crashes

Hirð splits failure into two kinds and never blurs them:

- **Domain errors** are values, tracked as `Exn<E>` in the effect row
  (`Exn<ParseError>`). The row is the checked, exhaustive list of a
  function's recoverable failures; a row without `Exn` cannot produce
  one.
- **Crashes** are process deaths: `crash!("msg")` (alias `panic!`),
  request timeouts, resource failures. They are not in the row and
  cannot be caught by Hirð code — the supervisor restarts the
  process. `crash!` is typed `∀a. (String) → a`, so it fits any
  position: `if ok then value else crash!("impossible")`.

If a caller could meaningfully recover, it is an error; if only a
restart makes sense, it is a crash. The normative treatment is in
[`error-model.md`](error-model.md).

## Working with the CLI

| Command | What it does |
|---|---|
| `hird check <input>` | Parse and type-check; print diagnostics. |
| `hird build <input>` | Check, emit Erlang source, compile with `erlc` into `_build/hird/`. |
| `hird run <input>` | Build, then run on BEAM via the boot module. |
| `hird emit-ast <input> [--json]` | Dump the typed IR of one file. |
| `hird emit-effect-graph <input> [--json]` | Dump the actor/effect graph: actors, mailboxes, per-handler rows, supervisors, tools. |
| `hird demo` | Record one run of the built-in demo planner, replay it against variants of the program, print the divergence table. |

`<input>` is a `.hird` file or a directory of modules. `hird run`
needs exactly one `fn main() → ()` with no parameters and no residual
`Tool<…>` effects — handle them with a `handle` block, or keep tool
calls inside actors and `install` their implementations. Other
effects (`Install`, `Supervise`, `Stand`, `Clock`, `Send`, `Await`, …)
may remain on `main`.

Diagnostics carry stable codes: `P####` for parse errors (catalogued
in [`parser-diagnostics.md`](parser-diagnostics.md)) and `C####` for check errors. The wire
format of the audit stream is specified in
[`tool-effects.md`](tool-effects.md).

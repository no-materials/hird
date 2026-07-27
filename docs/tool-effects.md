# Tool Effects

Tool effects are Hirð's primitive for auditable, structured invocations of
external operations — often non-deterministic, often LLM-mediated. This
document explains what they are, how declarations desugar, how handlers
interact with them, and it is the **normative specification** of the audit
log wire format and of replay semantics. The reference implementation is
the `wire` module of `hird-check`; the golden files under `conformance/`
are the byte-exact contract any other implementation must reproduce.

## Why tool effects exist

An agent system is, at its core, a program that calls tools: read a
repository, call an LLM, create a ticket, run a command. Three things go
wrong when those calls are ordinary functions:

1. **They disappear.** Nothing in a signature distinguishes a function that
   calls an external service from one that formats a string. Tool effects
   put every external invocation in the effect row, where callers and
   tooling can see it.
2. **They can't be audited.** Ad-hoc logging captures whatever the author
   remembered to log. A tool effect produces a complete, typed invocation
   record — tool name, structured arguments, structured result, timestamp,
   caller — derived by the compiler from the declaration.
3. **They can't be replaced.** Testing against live services is slow and
   flaky. Because a tool call is an effect, a `handle` block swaps its
   implementation — a mock, a recorder, a replayer — without touching the
   code under test.

## Tool declarations

```
tool ReadRepo : { path: Path } → RepoState
tool CreateTicket : { title: String, body: String } → TicketId
tool LLMCall<t> : { prompt: Prompt, schema: Schema<t> } → t ! {Exn<ParseError>}
```

A declaration names the tool (`PascalCase`), gives its input as a record
type, its output type, and optionally a trailing effect row for the other
effects an implementation may perform (typically `Exn<…>` for its failure
mode). A tool may be generic (`LLMCall<t>`); its type parameters scope over
the whole signature.

Each declaration creates three things:

1. **An effect.** `Tool<ReadRepo>` — one shared, built-in `Tool` effect
   applied to the tool's marker type, valid in any effect row and
   composing with other effects normally.
2. **A function.** The tool's name in `snake_case`, with acronym runs kept
   whole (`ReadRepo` → `read_repo`, `LLMCall` → `llm_call`):

   ```
   read_repo : { path: Path } → RepoState ! {Tool<ReadRepo>}
   llm_call : ∀t. { prompt: Prompt, schema: Schema<t> } → t
     ! {Tool<LLMCall>, Exn<ParseError>}
   ```

   Calling it performs the tool effect, so every caller must carry
   `Tool<…>` in its row (or handle it).
3. **An invocation record.** A compiler-derived record type describing one
   call, kept in the checker's side-table under the generated name
   `ReadRepoInvocation`:

   ```
   { tool: String, args: { path: Path }, result: RepoState,
     timestamp: Timestamp, caller: CallerId }
   ```

   `args` and `result` are projected from the signature; `tool`,
   `timestamp`, and `caller` are fixed schema fields injected when a
   record is produced. The record is what the audit log serialises.

### Wire-representability

Tool args and results cross the audit-log wire boundary, so the checker
rejects tool signatures containing:

- **function types** — not serialisable, directly or nested inside a
  declared type's constructors;
- **opaque capability types** — a capability decoded from a log would be a
  forged capability.

Type parameters of a generic tool are fine: each concrete invocation is
validated value-by-value at the wire layer.

## Handlers and tool effects

Tool effects are handled like any other effect, DI-style: a `handle` block
provides a function per handled effect, and calls within the block route
through it.

```
handle {
  Tool<ReadRepo> → mock_read_repo,
  Tool<CreateTicket> → mock_create_ticket,
} in planner_main(config)
```

The handler must be a function from the tool's input record to its output
type; the checker enforces this against the tool's declared signature, and
a mismatched handler is a compile error. The handler's own effect row is
not constrained — a mock may be pure even when the tool declares a
trailing row. The block's row is the body's effects minus the handled
effects plus the handlers' own effects — so replacing a tool with a pure
mock *removes* `Tool<…>` from the row, and replacing it with a logging
implementation *trades* it for the logger's effects. Standard patterns:

- **Mocking**: handle `Tool<X>` with a pure function; tests run without
  the external service and the enclosing code becomes pure.
- **Dry-run**: handle mutating tools with recorders that return canned
  results, leaving read-only tools live.
- **Audit interception**: wrap the real implementation with one that also
  emits an invocation record (below).
- **Replay**: substitute a handler that reads a log instead of executing
  (below).

### The audit sink is a capability

Where invocation records go is not ambient configuration. The sink is an
opaque capability value (`AuditSink`), passed in like any other
capability, and emission is a handler *wrapping* the tool effect — visible
in the effect row, never implicit in tool dispatch:

```
fn audited_read(
  sink: AuditSink,
  emit: { sink: AuditSink, line: String } → () ! {Audit<sink>},
  args: { path: Path }
) → RepoState ! {Audit<sink>, Tool<ReadRepo>} =
  let logged = emit({ sink: sink, line: "ReadRepo" }) in read_repo(args)

fn plan_audited(sink: AuditSink, emit: …, p: Path)
  → RepoState ! {Audit<sink>, Tool<ReadRepo>} =
  handle { Tool<ReadRepo> → λargs → audited_read(sink, emit, args) } in
    read_repo({ path: p })
```

`Audit<sink>` is a capability effect: it carries the sink parameter's
type, so the row records that this function emits audit records and what
it emits them through. Omit the `sink` parameter and the program does not
type-check — there is no ambient sink to reach for. The default sink
serialises records as JSON lines, one record per line, in the canonical
form below.

## Audit log format (normative)

The audit log is JSON lines: one record per tool invocation, one
invocation per line, `\n`-terminated. This section is the specification;
`conformance/v1/` holds golden files that any producer must reproduce byte
for byte.

### Envelope

Fields appear in exactly this order:

| field            | type            | notes                                    |
|------------------|-----------------|------------------------------------------|
| `schema_version` | integer         | required; this document specifies `1`    |
| `tool`           | string          | the tool's declared name                 |
| `args`           | value           | the input record, encoded as below       |
| `result`         | tagged object   | `{"ok":…}` or `{"err":…}`                |
| `timestamp`      | string          | RFC 3339 UTC, millisecond precision      |
| `caller`         | string          | `"Module.function"`                      |
| `meta`           | object          | optional; omitted entirely when absent   |

```json
{"schema_version":1,"tool":"ReadRepo","args":{"path":"/home/user/repo"},"result":{"ok":{"files":[],"status":"clean"}},"timestamp":"2026-05-22T12:00:00.000Z","caller":"Planner.plan_repo","meta":{"duration_ms":42}}
```

- **`result` is tagged.** Failed invocations are first-class:
  `{"err":<value>}` carries the error value from the tool's declared
  `Exn<…>` row. A log replays failures as faithfully as successes.
- **`timestamp` and `caller` are injected.** There is no ambient clock;
  the recording handler supplies both. The timestamp form is exactly
  `2026-05-22T12:00:00.000Z` (millisecond precision, `Z` offset). The
  caller is `Module.function`; inside generated actor callbacks it takes
  the actor form (`"Planner.init"`, `"Planner.handle_msg/PlanRepo"`).
  Decoders treat `caller` as an opaque string; no other form is defined.
- **`meta` is observer-populated.** Transport metadata (`duration_ms`,
  retries, trace ids) belongs to whoever recorded the invocation, not to
  the compiler-derived record, whose five fields are `tool`, `args`,
  `result`, `timestamp`, `caller`. `meta` values are plain JSON
  (self-describing), keys sorted.

### Value encoding

Encoding is canonical — for a given value there is exactly one byte
sequence: no whitespace, deterministic ordering. It is injective *per
type*; decoding is type-directed against the tool's signature and
validates shape, labels, constructor names, and arities.

| Hirð value        | JSON                                                  |
|-------------------|-------------------------------------------------------|
| unit `()`         | `null`                                                |
| `Int`             | integer, exact within `i64`                           |
| `Float`           | shortest round-trip decimal, plain notation, finite   |
| `String`          | JSON string (`"` `\` and controls escaped, rest verbatim) |
| list              | array                                                 |
| tuple             | array of fixed arity                                  |
| record            | object, keys in sorted label order                    |
| ADT value         | `{"ctor":"Name","args":[…]}`, fields in that order    |

- `Bool` is an ADT like any other: `{"ctor":"True","args":[]}`. No special
  case.
- NaN and the infinities are **not wire-representable**; encoding them is
  an error.
- Float canonical form is the shortest decimal string that round-trips,
  written in plain (non-exponent) notation; integral floats print without
  a fractional part (`1`, not `1.0`).

### Upgrade path

v0.1 provides structured, deterministic, diffable logs — no
tamper-proofing. `schema_version` is required on every record precisely so
later versions can add content addressing, record chaining, or signatures
without ambiguity. A decoder for version 1 must reject other versions.

## Replay semantics

A log contains full arguments and tagged results for every invocation —
enough to re-run a program without its tools. Replay is not a language
mode; it is a handler choice:

- a **live handler** calls the real tool;
- a **replay handler** reads the log and returns logged values.

The core of replay is a pure function:

```
(log, position, tool, args) → Result<result, Divergence>
```

Matching is **strict sequential**: the record at `position` must have the
same tool and byte-identical arguments as the request, and its logged
result — `ok` or `err` — is returned; the caller advances the position by
one. Any mismatch is a hard error carrying a structured `Divergence`
value: the log is exhausted, the tool differs, or the args differ.

Keyed matching (find a record by tool and args anywhere in the log) and
live fall-through (execute for real on a miss) are deliberately excluded:
both reintroduce the nondeterminism replay exists to remove. If a program
diverges from its log, the honest answer is an error, not a guess.

*Provisional*: the shape of divergence reporting (what context a
`Divergence` carries and how it renders) may be refined after experience
with real runs; strict-sequential matching itself is settled.

## Tool effects vs regular effects

| | regular effect | tool effect |
|---|---|---|
| declared by | `effect Log`, `effect Exn<t>` | `tool ReadRepo : …` |
| carries | a name and type arguments | a full operation signature |
| generates | nothing | function + invocation record |
| in a row | `{Log}` | `{Tool<ReadRepo>}` — one shared `Tool` effect per marker |
| handled by | DI-style `handle` | same, uniformly |
| audit | nothing intrinsic | structured invocation record, wire format |
| signature constraint | none | args/result must be wire-representable |

A tool effect *is* an effect — same row algebra, same handlers, same
polymorphism. What the `tool` declaration adds is structure: a typed
operation signature the compiler can derive records from, and a wire
format those records serialise to.

## Declaring tools for LLM-mediated operations

LLM calls are schema-typed. The standard tool is:

```
tool LLMCall<t> : { prompt: Prompt, schema: Schema<t> } → t ! {Exn<ParseError>}
```

Guidance:

- **Let the schema fix the type.** The `Schema<t>` argument ties the
  result type to the call site through ordinary unification:
  `llm_call({ prompt: p, schema: ticket_schema })` has type `Ticket`. Do
  not declare LLM tools returning raw `String` and parse downstream — that
  discards the typing story and hides the failure mode.
- **Declare the failure mode.** A response that does not conform to the
  schema raises `Exn<ParseError>`. Callers see it in the row and must
  handle it or carry it.
- **Keep args wire-representable and meaningful.** The invocation record
  is your audit trail of what the model was asked and what it answered;
  structure prompts and schemas as declared types, not opaque blobs.
- **Mock in tests, replay in audits.** A handler substitutes a canned
  response for `Tool<LLMCall>` in tests; a replay handler re-serves a
  recorded interaction deterministically, failures included.

## Example: the planner demo's tools

```
tool ReadRepo : { path: String } → { files: List<String>, status: String }
tool CreateTicket : { title: String, body: String } → TicketId
```

`read_repo` performs `Tool<ReadRepo>`; `create_ticket` performs
`Tool<CreateTicket>`. Both effects share the `Tool` head in a row:

```
fn plan(p: String) → TicketId ! {Tool<ReadRepo>, Tool<CreateTicket>} =
  let state = read_repo({ path: p }) in
  create_ticket({ title: "Flaky CI", body: "Investigate flaky CI on main" })
```

A run of the planner under a recording handler produces the demo's
headline output, one line per invocation (this is
`conformance/v1/planner_log.jsonl`):

```json
{"schema_version":1,"tool":"ReadRepo","args":{"path":"/home/user/repo"},"result":{"ok":{"files":[],"status":"clean"}},"timestamp":"2026-05-22T12:00:00.000Z","caller":"Planner.plan_repo","meta":{"duration_ms":42}}
{"schema_version":1,"tool":"CreateTicket","args":{"body":"Investigate flaky CI on main","title":"Flaky CI"},"result":{"ok":{"ctor":"TicketId","args":["TCK-42"]}},"timestamp":"2026-05-22T12:00:01.250Z","caller":"Planner.plan_repo"}
{"schema_version":1,"tool":"HttpGet","args":{"url":"https://ci.example/status"},"result":{"err":{"ctor":"HttpError","args":[503,"service unavailable"]}},"timestamp":"2026-05-22T12:00:02.000Z","caller":"Planner.check_ci","meta":{"duration_ms":1200}}
```

Reading it back through the replay handler re-runs the planner
deterministically: the same repository state, the same ticket id, and the
same HTTP failure, with no repository, ticket system, or network anywhere
in sight.

## v0.1 limitations

- The standard tools (`llm_call`, `http_get`, `http_post`, `read_file`,
  `write_file`, `shell`) are proven by type-checked fixtures but are not
  importable by user programs until standard-library resolution lands.
- There is no backend yet: records and replay are exercised by the
  reference implementation and its conformance suite, not by generated
  code.
- The audit log has no tamper-proofing (content addressing, chaining,
  signatures); the `schema_version` field reserves the upgrade path.

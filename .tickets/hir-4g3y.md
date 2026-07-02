---
id: hir-4g3y
status: closed
deps: [hir-t1cj]
links: [hir-x6cx]
created: 2026-05-22T21:39:26Z
type: task
priority: 1
assignee: nomaterials
parent: hir-jt39
tags: [phase-6, tools]
---
# Tool declarations and invocation records

Implement tool declarations as a special class of effect and the compiler-
generated invocation record types.

**Tool declaration syntax**:
```
tool ReadRepo : { path: Path } -> RepoState
tool CreateTicket : { title: String, body: String } -> TicketId
tool LLMCall<T> : { prompt: Prompt, schema: Schema<T> } -> T ! {Exn ParseError}
tool HttpGet : { url: Url, headers: Headers } -> HttpResponse ! {Exn HttpError}
```

A tool declaration creates:
1. An effect: Tool<ReadRepo>, Tool<CreateTicket>, etc.
2. A function: read_repo(args) -> RepoState ! {Tool<ReadRepo>}.
3. An invocation record type (compiler-generated):
   ```
   type ReadRepoInvocation = {
     tool: "ReadRepo",
     args: { path: Path },
     result: RepoState,
     timestamp: Timestamp,
     caller: CallerId,
   }
   ```

**Integration with effect system**:
- Tool<X> is a valid effect in any effect row.
- Tool effects compose with other effects normally.
- Tool effects are handleable via DI-style handlers (from Phase 5).

**Standard library tool declarations** (in a prelude or standard module):
- llm_call<T>: schema-typed LLM invocation (resolves OD2).
- http_get, http_post: HTTP operations.
- read_file, write_file: filesystem operations.
- shell: shell command execution.

Each standard tool has a proper invocation record type.

This ticket resolves **OD2 (LLM call typing)**: confirm schema-typed approach.

## Acceptance Criteria

- tool declaration syntax parsed, type-checked, and registered.
- Each tool creates an effect, a function, and an invocation record type.
- Tool effects integrate with the general effect row system.
- Standard library tools declared: llm_call, http_get, http_post, read_file,
  write_file, shell.
- Invocation record types are compiler-generated with correct fields.
- OD2 documented in DECISIONS.md.
- Snapshot tests: tool declaration, tool call in effect row, tool invocation
  record structure, standard library tool types.
- At least 8 snapshot tests.


## Notes

**2026-07-01T13:47:08Z**

Design decisions locked (D1/D2 via council; D3–D6 by owner intuition). Recorded
in DECISIONS.md as ADR-015 (tool declarations); OD2 resolved and removed from the
open-decision-slots table.

D1 — Invocation record representation: a compiler-DERIVED, named STRUCTURAL record
held in a checker side-table (keyed by generated name, e.g. ReadRepoInvocation),
NOT a single-constructor ADT wrapper and NOT a new named-record language feature.
Fields: { tool: String, args: <input>, result: <output>, timestamp: Timestamp,
caller: CallerId } — args/result projected from the signature; tool/timestamp/
caller are fixed schema fields (timestamp and caller are runtime-injected, so the
record is not purely signature-derived). Snapshot the derived record by name to
satisfy the "compiler-generated record with correct fields" AC; no first-class
value type is required because v0.1 has no in-language consumer (mock handlers,
IR-only, no backend). Representation stays unlocked — the audit-log sibling pins
the JSON wire contract and may promote it additively with no user migration.

D2 — Standard-library boundary: the tool-declaration MECHANISM lands here (in the
compiler). The six standard tools + supporting types (Prompt, Schema<t>, Path,
Url, Headers, HttpResponse, Timestamp, CallerId, TicketId, RepoState) + the
Tool/Exn effects are declared in .hird TEST FIXTURES fed through check_str — NOT
hardcoded built-ins (ossifies / smuggles a prelude past ADR-010) and NOT an
implicit prelude module (reopens ADR-010). The snapshot harness already asserts
fixtures parse and type-check, so a rotted fixture fails CI. Fixtures graduate to
a real prelude unchanged when ADR-010 is superseded. Standard tools are proven by
fixtures but are NOT importable by user programs in v0.1 — documented limitation.

D3 — Desugaring model: `tool ReadRepo` registers (a) a nullary marker type
ReadRepo so Tool<ReadRepo> resolves (effect args are types), (b) the function
read_repo : (args) → result ! ({Tool<ReadRepo>} ∪ declared_row), generalised and
bound like an ADT constructor, (c) the derived invocation record (D1). ONE
built-in Tool effect at arity 1, parameterised by the marker — not a per-tool
effect. Naming: use Tool<LLMCall> (marker = the tool's own name); the OD2 draft's
`Tool<LLM>` is superseded by this convention.

D4 — Generic tools & trailing rows (the shape of OD2): parse_tool_decl must gain
(i) an optional `<t,…>` type-param list (currently missing) and (ii) an optional
trailing `! {row}`. Generic tools bind their params in a closed elaboration scope
and generalise (reuse the ADT type-param path); the trailing row unions into the
function's row. llm_call<t> is the only generic standard tool.

D5 — `tool` field type: String for v0.1 (no singleton/literal types); value is
compiler-fixed. Narrowable later by a literal-type feature without touching the
other fields.

D6 — Handler-signature checking: OUT OF SCOPE here. ADR-013 deferred
signature-directed handler checking "until tool declarations introduce those
signatures"; those signatures now exist, but validating handler arms against them
is a separate follow-up so this work stays declarations-plus-record. Tracked
separately (linked).

AC interpretation: "invocation record types compiler-generated with correct
fields" and the "record structure" snapshot are met by the named side-table
structural record — no requirement that it be a user-referenceable type. Suggested
≥8 snapshots: basic tool decl; generic tool decl; tool call in an effect row; tool
with a trailing Exn row; derived record structure; handler substitution over a
tool effect; standard-library tool types (fixture); unknown-effect / wrong-arity
errors.

**2026-07-02T06:32:45Z**

Implemented per the locked design (ADR-015, D1-D6).

Landed in commit 79c69ca:
- Grammar: record types { name: Type } join the type-atom grammar (braces in
  type position are unambiguous); parse_tool_decl gains the optional <t,...>
  type-param list and optional trailing ! {row} (D4). New AST projections:
  TypeExpr::Record, ToolDecl::{type_params,input,output,effect_ann}.
- Checker: each tool registers a nullary marker type; binds the generated
  snake_case function (input) -> output ! ({Tool<Marker>} u declared_row),
  elaborated in a closed scope over the tool's type params and generalised
  like an ADT constructor (D3); derives the invocation record
  { tool: String, args, result, timestamp: Timestamp, caller: CallerId }
  into the new CheckedFile::invocation_records side-table keyed by
  NameInvocation (D1, D5). Tools occupy both namespaces for duplicate
  detection (marker type + generated function).
- Standard tools llm_call/http_get/http_post/read_file/write_file/shell +
  supporting types + Tool/Exn effects live in
  crates/hird-check/tests/fixtures/std_tools.hird, checked by the snapshot
  suite (D2); not importable by user programs in v0.1 as documented.
- Tests: 13 checker tests in tests/tools.rs (12 snapshots incl. basic decl,
  generic decl, call in row, trailing row union, record structure, handle
  substitution, std fixture, unknown-effect/wrong-arity/dup-param/collision/
  open-row errors) + 5 parser smoke snapshots + AST projection tests.
- OD2 remaining ACs: llm_call declaration reflects schema-typing; phrasebook
  gains an llm_call usage example.

Surface-syntax note: the epic/phrasebook wrote 'Exn ParseError' (juxtaposed);
every other parametric effect uses angle brackets and D4 listed no row-grammar
work, so the exception row is spelled Exn<ParseError> with 'effect Exn<t>'
declared in fixtures. Phrasebook aligned.

Handler-signature checking stays deferred per D6 (hir-uvui); record wire
format / audit sink is hir-jgs1.

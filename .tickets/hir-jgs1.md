---
id: hir-jgs1
status: closed
deps: [hir-4g3y]
links: [hir-yum3, hir-v3pv]
created: 2026-05-22T21:39:42Z
type: task
priority: 1
assignee: nomaterials
parent: hir-jt39
tags: [phase-6, tools, audit]
---
# Audit log integration and tool effect docs

Define the tool-invocation wire schema, build its reference serializer and
replay function, prove the language-level side with type-checked fixtures,
and write the comprehensive tool-effects documentation.

There is no backend or runtime yet (ADR-002; `hird-codegen` is a stub), so
"implement the audit log" means: the wire format is specified and locked, a
Rust reference implementation produces and consumes it byte-exactly, and the
golden files it snapshots become the conformance suite the future Erlang
runtime must pass. Nothing here waits on execution.

**Wire schema** (locked; see Locked decisions below):
- Every tool effect invocation is described by one JSON-lines record.
- Envelope fields in fixed order: `schema_version`, `tool`, `args`, `result`,
  `timestamp`, `caller`, `meta`.
- `result` is tagged: `{"ok": <value>}` or `{"err": <value>}` — failed
  invocations are first-class in the schema.
- `meta` is an optional observer-populated object for transport metadata
  (e.g. `duration_ms`); it is not part of the compiler-derived invocation
  record, whose five fields stand as specified in ADR-015.
- Record format:
  ```json
  {
    "schema_version": 1,
    "tool": "ReadRepo",
    "args": { "path": "/home/user/repo" },
    "result": { "ok": { "files": [], "status": "clean" } },
    "timestamp": "2026-05-22T12:00:00.000Z",
    "caller": "Planner.plan_repo",
    "meta": { "duration_ms": 42 }
  }
  ```
- The audit sink is itself a capability: `AuditSink` is passed in, not ambient,
  and audit emission is a handler wrapping the tool effect — visible in the
  effect row, never implicit in tool dispatch.

**Replay semantics** (resolves OD4):
- A replay handler, given a log, returns logged values for tool effects
  instead of re-executing — deterministic, suitable for testing and audit.
- Replay is strict sequential: records are matched in order; a divergence
  (tool or args mismatch, exhausted log) is a hard error carrying a
  structured `Divergence` value. Keyed matching and live fall-through are
  out of scope — they reintroduce nondeterminism.
- The choice between replay and re-execute is a handler decision, not a
  language feature — a replay handler reads the log; a live handler calls
  the real tool.
- Core is a pure function: `(log, position, tool, args) -> Result<value, Divergence>`.

**Documentation** (docs/tool-effects.md):
- What tool effects are and why they exist.
- How tool declarations work (syntax, generated types, invocation records).
- How handlers interact with tool effects (DI-style replacement, mocking).
- Audit log format specification (the wire schema above, normatively).
- Replay semantics (OD4 resolution).
- Comparison with regular effects.
- LLM-specific guidance: how to declare tools for LLM-mediated operations.
- Examples: the planner demo's tool declarations annotated.

This ticket resolves **OD3 (Audit log fidelity)** and **OD4 (Replay semantics)**.

## Locked decisions (council, 2026-07-02)

**A — Shape and placement.** Rust reference serializer + pure replay function
+ type-checked `.hird` fixtures. No interpreter (contradicts ADR-002 staging);
no docs-only rescope. Code lives as a `wire` module in `hird-check`, beside
the derived invocation records — no new crate (ADR-014 lesson); extract to
its own crate only when the Erlang emitter exists as a second consumer.
Snapshots are language-agnostic golden `.json` files in a versioned
conformance directory; the Erlang runtime must later reproduce them
byte-exact, making the Rust implementation the conformance oracle rather
than a rival source of truth.

**B1 — Value encoding.** ADT values `{"ctor": "Name", "args": [...]}`; unit
`null`; records as JSON objects with fields in sorted label order (records
are structural); the envelope uses the fixed order above. Integers exact
within i64; floats shortest-round-trip; NaN/Infinity are not
wire-representable. Encoding is injective per type; decode validates against
the tool's signature (round-trip property tested). Wire-representability of
tool args/results is a checker-enforced constraint: function types and
capabilities are rejected in tool signatures.

**B2 — duration_ms.** Not in the compiler-derived record; lives in the
optional `meta` envelope field. `schema_version` is required on every record.

**B3 — Replay.** Strict sequential, hard error on divergence (locked).
Only divergence-reporting ergonomics are provisional pending real runs.

**B4 — Caller ID.** `"Module.function"` string in v0.1. The actor form
(`"Planner.handle_msg/PlanRepo"`) is a documented provisional extension
gated on Phase 7, absorbed via a `schema_version` bump — not encoded today.

**B5 — JSON.** Hand-rolled canonical writer: deterministic byte-exact
output, no whitespace, ordering per B1, `no_std`-compatible. No serde_json.

**B6 — Sink threading.** Explicit only. `AuditSink` is a capability
parameter; emission is a handler wrapping the tool effect. A fixture must
fail to type-check when the sink parameter is omitted.

**Timestamps.** RFC 3339 UTC with millisecond precision
(`2026-05-22T12:00:00.000Z`). Tests inject timestamps and caller IDs; no
ambient clock anywhere.

Provisional items (divergence-reporting ergonomics, actor caller form) are
marked provisional in docs/tool-effects.md and in the OD3/OD4 ADR when it is
written during implementation.

## Acceptance Criteria

- The reference serializer produces structured JSON invocation records
  matching the wire schema, snapshot-tested as golden `.json` files.
- Checker enforces wire-representability of tool args/results (function
  types and capabilities rejected in tool signatures), with a test.
- Failure invocations are encodable (`result` tagging) and covered by tests.
- Audit sink is capability-based (AuditSink parameter, not ambient);
  a fixture omitting the sink parameter fails to type-check.
- Default audit sink writes JSON lines.
- Replay function: given a log, returns logged values for tool effects;
  strict sequential; structured `Divergence` error on mismatch; round-trip
  (encode/decode) property tested.
- docs/tool-effects.md written with all sections listed above.
- OD3 and OD4 documented in DECISIONS.md.
- Snapshot tests: invocation record format, audit log output for a sequence
  of tool calls, replay returning logged values, divergence error.
- At least 6 snapshot tests.

## Notes

**2026-07-02T13:18:14Z**

Design council convened 2026-07-02; ticket body amended with the locked
decision set (see 'Locked decisions' section). Key changes vs the original
body: duration_ms moved out of the invocation record into an optional 'meta'
envelope field (record contradicted ADR-015's five-field derived record);
result field is now tagged ok/err so failures are first-class; caller ID is
Module.function in v0.1 (actor form deferred to Phase 7 behind
schema_version); replay locked to strict-sequential with hard Divergence
error (keyed matching / live fall-through rejected as nondeterminism);
implementation is a wire module in hird-check plus golden-file conformance
snapshots — no new crate, no interpreter.

**2026-07-03T09:44:36Z**

Implemented per the locked decision set; landed in commits 36e0aff (checker
wire-representability), bea7971 (wire module + conformance suite), f388ddf
(docs + ADR-016).

- Checker: C0032 rejects function types and opaque capabilities in tool
  signatures, walking nested ADT constructor fields (visited-set bounded);
  generic tool params pass and are validated per-value at the wire layer.
- Wire module (hird-check::wire, no_std): WireValue + canonical hand-rolled
  JSON writer (fixed envelope order, sorted labels, shortest round-trip
  plain-notation floats, NaN/Inf rejected, i64-exact ints, ADTs as
  {"ctor","args"}, unit null, Bool uniform); tagged ok/err results;
  optional self-describing meta; type-directed decoder validating against
  ToolWireSig (args/result/Exn error types) via an AdtTable derived from
  CheckedFile; JsonLinesSink default sink; pure strict-sequential
  replay(log, position, tool, args) -> Result<&ToolResult, Divergence>
  with Exhausted/ToolMismatch/ArgsMismatch variants.
- Conformance: conformance/v1/{read_repo_ok,create_ticket_ok,http_get_err}.json
  + planner_log.jsonl, byte-exact-tested both directions (encode reproduces
  bytes; decode against checked signatures round-trips). Erlang runtime
  later reproduces these files exactly.
- Fixtures: audit_sink.hird proves capability threading (Audit<sink> visible
  in rows through a handler wrapping Tool<ReadRepo>); the negative fixture
  omitting the sink fails with C0003 (no ambient sink), snapshot-tested.
- Property test: encode/decode round-trip over randomly generated well-typed
  values (proptest, type-directed generation incl. generic Opt ADT).
- docs/tool-effects.md written (all ticket sections; normative wire spec;
  provisional items marked: divergence-reporting ergonomics, actor caller
  form). ADR-016 records OD3/OD4; open-slots table updated.

12 snapshot/golden tests + 27 wire unit tests + property test; workspace
fmt/clippy(-D warnings)/tests all green (521 passed).

# The Audit Stream as Evidence

Every tool invocation a Hirð program performs is recorded, unconditionally,
in a canonical wire format. This document is for the reader who needs to
*rely* on that stream: as a record of what an automated system did, as the
fixed input of a reproducible run, or as the oracle of a regression test.
It states what the stream guarantees today, what it deliberately does not,
how a run is recorded, replayed and retained, and under what policy the
format may change.

The format itself and the semantics of replay are specified normatively in
[`tool-effects.md`](tool-effects.md); nothing here restates them.

## Why the stream is semantics, not instrumentation

An audit trail bolted onto a runtime is only as complete as the call sites
that remembered to use it. Hirð's is a property of the language instead:

- **One call path.** Every tool call site in generated Erlang emits
  `hird_tool_dispatch:call/4`, never a direct handler invocation, and the
  dispatcher captures the record around the invocation. There is no flag
  that disables it and no route around it: a mocked call, a live call and a
  replayed call produce the same record.
- **Compiler-derived records.** The record's shape comes from the `tool`
  declaration — name, arguments, result, timestamp, caller — not from
  author-written logging, so it cannot drift from what the program does.
- **Encodability is checked.** A tool whose arguments or result contain a
  function type or an opaque capability is a compile error (`C0032`),
  walking through nested constructor fields. Every declarable tool's
  records encode, and a decoded log can never mint a capability.
- **Configured at the boundary.** Where records go is decided once, where
  the program is started: the generated boot module opens the sink before
  `main` and flushes it after, at stdout by default and at a file with
  `--audit-file`. Nothing in the language reaches the sink, so no library
  can redirect or silence the stream mid-run.

So "audited" is not a property of a deployment's configuration that an
operator could get wrong. That is what makes the stream usable as evidence.

## What the stream guarantees today

Each is an implemented mechanism with tests behind it — in the Rust
reference implementation (`hird-check`'s `wire` module), in the Erlang
runtime, or in both.

- **Every completed call is recorded.** An invocation that returns, or that
  fails with a domain error, produces one record: tool name, full
  arguments, tagged result, timestamp, caller.
- **Failures are first-class.** A domain failure is recorded as
  `{"err":<value>}`, carrying the error value from the tool's declared
  `Exn<…>` row. A failed invocation replays as faithfully as a successful
  one.
- **Canonical bytes.** For a given record there is exactly one byte
  sequence. Two independent implementations are held to it: the Rust
  encoder is the oracle that generated `conformance/v1/`, and the Erlang
  runtime reproduces those files byte for byte in its own suite, run in CI
  on every change. Logs are therefore diffable, and equality between two
  logs is a meaningful comparison rather than a formatting accident.
- **Decoding is type-directed.** A record is validated against the tool's
  signature — shape, labels, constructor names, arities, and
  `schema_version` — and a record that does not type is rejected. Neither
  garbage nor a record from an unknown tool round-trips silently.
- **The sink appends.** A file sink opens the log for append, so no restart
  and no second run truncates an existing one. Re-recording over a log
  means removing it first, deliberately.
- **Replay is exact and total.** A recorded log replays by strict
  sequential matching; any mismatch is a hard error carrying a structured
  divergence, and a log the run did not read to the end fails the run.
  There is no keyed matching and no live fall-through.

## What it does not guarantee

Stated plainly, because anything beyond this list is a claim the
implementation does not make.

- **No tamper-evidence.** Version 1 has no record chaining, content
  addressing or signatures. A log is exactly as trustworthy as the storage
  holding it and the process that wrote it: whoever can write the file can
  rewrite history undetectably. `schema_version` is required on every
  record precisely to reserve that upgrade path. Until it is taken,
  integrity is an operational property — append-only storage, write-once
  media, an external shipper — and not a property of the format.
- **No record of a crash.** A record is written after the invocation
  settles: on return, or on the domain-error throw. Anything else — an
  unhandled tool, a bug in a handler, a killed process — propagates
  untouched and unrecorded. The stream says what the program did, not
  everything it attempted, so a run's last attempted call may be missing
  from its own log. Crash evidence is the supervisor's business.
- **Durability is best-effort.** Records reach the sink asynchronously and
  the generated boot module flushes it before the run returns, so a normal
  exit loses nothing. A hard halt of the VM can drop records still queued.
- **Arrival order, not causal order.** The stream is ordered by arrival at
  the sink. Calls from one process appear in call order; concurrent actors
  interleave, and a record carries no ordering token beyond its
  millisecond timestamp.
- **Timestamps are not attested.** They are injected at the recording site
  from the runtime's system clock at millisecond precision. There is no
  trusted time source, no monotonicity guarantee across a clock
  adjustment, and no external attestation.
- **No redaction.** Arguments and results are recorded in full; the record
  is derived from the declaration, which has no notion of a sensitive
  field. A tool's signature is therefore its disclosure boundary: what a
  handler holds privately — an API key, a session — is never recorded
  unless the declaration puts it in the arguments, and an opaque
  capability cannot be put there at all.
- **Tool calls only.** Non-tool effects, actor messages and supervisor
  restarts are not on the stream. `hird emit-effect-graph` describes those
  statically; nothing records them at runtime.
- **No retention machinery.** One run appends to one file. There is no
  rotation, compaction, expiry or shipping, deliberately: the sink writes
  canonical lines to a file, and everything downstream of that file is the
  operator's.
- **`meta` is unvalidated.** It is self-describing JSON populated by
  whoever recorded the invocation — the v0.1 runtime emits none — and
  nothing compiler-derived may move into it without a version bump.

One thing is provisional rather than absent: the shape of a divergence
report — what context it carries and how it renders — may be refined with
experience. Strict-sequential matching itself is settled.

## Recording, retaining, replaying

The mechanics are in
[`tool-effects.md`](tool-effects.md#recording-and-replaying-a-run); this is
the same workflow read as an evidence chain.

1. **Record.** `hird run prog.hird --audit-file run.jsonl` routes the
   stream to a file instead of stdout. The handlers installed in the
   program are irrelevant to whether the stream exists — only to what the
   recorded results say.
2. **Retain.** The file is the artifact, and canonical bytes are what make
   it one: a stable byte sequence to checksum, sign or ship. The format
   gives you something worth attesting; the attestation is yours to apply.
3. **Replay.** `hird run prog.hird --replay run.jsonl` serves every tool
   call from the log. The log is decoded up front against the program's own
   tool signature tables, so a log naming a tool the program does not
   declare, or holding a record of another `schema_version`, fails to load
   rather than half-replaying. During the run the cursor outranks every
   `handle` and `install` block: no tool runs and no service is contacted.
4. **Compare.** A replayed run audits the stream it consumed, with fresh
   timestamps, so `--replay` and `--audit-file` together are a round trip.
   Compare a replayed log to its original with the timestamp values
   blanked — the one field a replay cannot reproduce.
5. **Regress.** A checked-in recording is a behavioral test with no oracle
   to maintain. The demo suite replays `demo/agent_planner.golden.jsonl` in
   the CI job that installs Erlang, so the build fails the moment the
   program's decisions drift from the recorded ones.

## Wire-format stability policy

`schema_version` is `1`. The golden files under `conformance/v1/` are the
contract; the Rust encoder is the oracle that generated them, and the
Erlang runtime is a second implementation held to the same bytes.

A change requires a version bump when a version-1 decoder would misread
the result: the envelope's field set or field order, the encoding of any
value form, the tagging of `result`, the timestamp form, or a new field a
decoder must understand — tamper-evidence included, which is exactly why
the field exists.

A change needs no bump when version 1 already describes it: a new tool or
type (records are derived from declarations, and decoding is directed by
the signature); a new `caller` string form, since decoders treat `caller`
as opaque — this is how the actor form arrived; new `meta` keys, which are
observer-populated and unvalidated; and a new golden covering a case the
existing files do not reach.

Two consequences are worth naming. A decoder for version 1 must *reject*
any other version, so a bump is a hard break by design: old readers refuse
new logs instead of misreading them. And changing the bytes of an existing
golden is never a test update — it is either a format change, which needs
the bump, or a bug in the oracle, and both break every log already
recorded.

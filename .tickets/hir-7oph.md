---
id: hir-7oph
status: open
deps: [hir-zp13]
links: []
created: 2026-05-22T21:41:26Z
type: task
priority: 1
assignee: nomaterials
parent: hir-7rsf
tags: [phase-9, runtime, erlang]
---
# Erlang runtime support library

Write the hand-written Erlang runtime support library that Hirð-compiled
programs depend on.

**Modules in runtime/**:

1. **hird_tool_dispatch.erl** — tool effect dispatcher.
   - API: call(ToolName, Handlers, Args) — the shape every generated tool
     call site uses (ADR-022).
   - Looks up `{tool, ToolName}` in the threaded handler map; entries are
     binary funs fun(Args, Handlers).
   - On a miss, falls back to the hird_handlers registry; if that misses
     too, raises erlang:error({unhandled_tool, ToolName}).
   - Captures the invocation record (tool name, args, result, timestamp,
     caller) around every invocation — mocked or real — and sends it to the
     audit sink.

2. **hird_audit.erl** — audit log sink.
   - Accepts invocation records from the tool dispatcher.
   - Default output: JSON lines to a configured file or stdout.
   - API: start_link/1 (configure sink), log/1 (record an invocation).
   - Implemented as a gen_server for ordered writes.

3. **hird_handlers.erl** — the runtime default-handler registry.
   - Lexical handle blocks thread a handler map through calls (ADR-013/022);
     this module is the *process-independent* fallback the dispatcher
     consults on a map miss — where deployments and test harnesses install
     process-wide defaults (e.g. mocks seen by spawned actors).
   - Keys match the threaded map's scheme: {tool, name} / bare head atom.
   - API: install_handler/2, lookup_handler/1, with_handlers/2.
   - Whether spawn should also snapshot the spawner's in-scope map
     (ADR-020 §6) is decided here, against this registry.

4. **hird_sup_util.erl** — supervisor utility functions.
   - Helper for constructing child specs from Hirð declarations.
   - Default restart configuration.

5. **hird_types.erl** — runtime type utilities (if needed).
   - Pretty-printing of Hirð values for logging/debugging.
   - Invocation record construction helpers.

Keep the runtime small. Each module should be under 200 lines. The runtime is
a dependency, not a framework.

## Acceptance Criteria

- runtime/ contains: hird_tool_dispatch.erl, hird_audit.erl, hird_handlers.erl,
  hird_sup_util.erl.
- Each module compiles with erlc.
- hird_tool_dispatch correctly routes tool calls through handlers.
- hird_audit writes JSON-lines invocation records.
- hird_handlers installs and looks up effect handlers.
- Each module has Erlang -spec annotations on public functions.
- Each module is under 200 lines.
- At least one Erlang-level test per module (eunit or common_test).


## Notes

**2026-07-10T09:17:20Z**

ADR-022 pins the generated-code contract this library implements: hird_tool_dispatch:call(ToolName, Handlers, Args), map keys {tool, name} / bare head atoms, binary-fun entries, registry fallback then {unhandled_tool, _} crash. hird_handlers is the process-independent default registry, not process-dictionary storage (rejected by ADR-013). Body amended.

**2026-07-10T12:10:09Z**

Contract update from ADR-020 §6 (amended 2026-07-10): handler maps never
cross the spawn boundary — actor codegen (hir-1dvq) invokes init and
handler bodies with `#{}`, so every tool call inside a spawned actor
reaches the dispatcher with an empty map and resolves through this
library's default registry (or crashes {unhandled_tool, ...} per
ADR-022 §3). The registry is therefore the *only* mechanism for
supplying handlers to actors; test harnesses (hir-bxdd) install mocks
here. No snapshot variant of start_link needs to be supported.

**2026-07-27T09:09:14Z**

Implementation recommendations for the two open contract gaps:

1. Caller field: amend the dispatch contract to
   hird_tool_dispatch:call(ToolName, Caller, Handlers, Args) with
   Caller a codegen-supplied binary literal ("Module.function", or the
   ADR-016 provisional actor form "Planner.handle_msg/PlanRepo"). The
   emitter statically knows the enclosing function at every dispatch
   site, so this is a free literal argument and satisfies ADR-016's
   injected-never-ambient rule. Rejected: stacktrace inspection
   (fragile, slow) and process-dictionary context (hidden state,
   contra ADR-005). Requires a small emit.rs change and an ADR-022
   amendment in the same commit.

2. Audit encoding: type-directed against a generated signature table,
   never term-directed heuristics. Byte-exact conformance/v1
   reproduction cannot survive guessing ctor-tuple vs plain-tuple from
   raw terms. Codegen already holds every tool signature: emit a
   metadata table into the base module (tool atom -> original
   PascalCase wire name + wire type shape) that generated code
   registers with hird_audit at startup. This also fixes the lossy
   read_repo -> "ReadRepo" casing without reconstruction, and keeps
   the Erlang encoder structurally parallel to the Rust oracle so the
   goldens become a straight eunit fixture. Open sub-choice is
   placement only (base module vs sidecar); base module preferred --
   one artifact, no extra file plumbing in hird build.

Also: hir-z9rn emitted supervisor modules with inline child specs, so
nothing generated references hird_sup_util; keep it minimal or drop it
from the AC.

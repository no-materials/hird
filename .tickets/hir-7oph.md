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
   - Looks up the handler for a tool effect (from handler chain or default).
   - Invokes the handler with structured arguments.
   - Captures the invocation record (tool name, args, result, timestamp, caller).
   - Sends the record to the audit sink.

2. **hird_audit.erl** — audit log sink.
   - Accepts invocation records from the tool dispatcher.
   - Default output: JSON lines to a configured file or stdout.
   - API: start_link/1 (configure sink), log/1 (record an invocation).
   - Implemented as a gen_server for ordered writes.

3. **hird_handlers.erl** — DI handler installation machinery.
   - Install/lookup handler functions for effects.
   - If using process dictionary approach: put/get handlers keyed by effect name.
   - If using parameter threading: provide a handler-map data structure.
   - API: install_handler/2, lookup_handler/1, with_handlers/2.

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


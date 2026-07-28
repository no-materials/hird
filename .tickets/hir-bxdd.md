---
id: hir-bxdd
status: closed
deps: [hir-7oph, hir-y9jo, hir-1dvq, hir-z9rn, hir-shiv]
links: []
created: 2026-05-22T21:41:57Z
type: task
priority: 0
assignee: nomaterials
parent: hir-7rsf
tags: [phase-9, demo, milestone]
---
# v0.1 demo: supervised agent planner end-to-end

The v0.1 milestone: the supervised agent planner demo, end-to-end runnable on
BEAM.

**The demo program** (written in Hirð):

A Planner actor that:
1. Receives a PlanRepo(path) message with a repository path.
2. Reads repository state through Tool<ReadRepo> (tool effect).
3. Analyzes the repo state (pure computation).
4. Creates tickets through Tool<CreateTicket> (tool effect).
5. Logs progress through Log.
6. Handles GetStatus(ReplyTo<PlannerStatus>) for status queries.
7. Handles Shutdown for graceful termination.

A PlannerSup supervisor that:
1. Uses one_for_one restart strategy.
2. Supervises the Planner actor.
3. Restarts on crashes (tool timeout, etc.).

A test harness that:
1. Installs mock handlers for Tool<ReadRepo> (returns canned repo data).
2. Installs mock handlers for Tool<CreateTicket> (records but doesn't create).
3. Installs a capturing Log handler.
4. Sends PlanRepo to the planner.
5. Verifies expected tickets were "created" (via mock).
6. Verifies the audit log contains the expected invocation records.

A CLI invocation that:
1. `hird build demo/planner.hird` produces .erl and .beam files.
2. `hird run demo/planner.hird` runs the demo on BEAM.
3. `hird emit-effect-graph demo/planner.hird --json` produces queryable JSON.

The effect graph JSON output includes:
- Planner actor with effects {Tool<ReadRepo>, Tool<CreateTicket>, Log}.
- Planner message type: PlanRepo | GetStatus | Shutdown.
- PlannerSup supervisor with one_for_one strategy, child: Planner.
- Tool declarations with structured argument/return types.

**This is the deliverable that proves the v0.1 promise:**
> Python agent frameworks hide side effects in coroutine soup. Hirð makes every
> tool call, every actor message, every retry, and every supervisor boundary
> visible in the types and queryable by tooling.

## Acceptance Criteria

- demo/planner.hird exists as a complete Hirð source file.
- `hird check demo/planner.hird` passes with no errors.
- `hird build demo/planner.hird` produces .erl and .beam files.
- `hird run demo/planner.hird` runs on BEAM and produces output.
- `hird emit-effect-graph demo/planner.hird --json` produces correct JSON.
- Effect graph JSON contains: actor, effects, message type, supervisor, tool decls.
- Test harness with mock handlers passes: sends PlanRepo, verifies ticket creation
  and audit log entries.
- Audit log JSON contains invocation records for ReadRepo and CreateTicket calls.
- Generated Erlang is inspectable and corresponds to the Hirð source.
- The demo is documented in README.md with build/run instructions.


## Notes

**2026-07-10T09:17:31Z**

ADR-022 consequence: non-tool effects have no compiler-known operation in v0.1, so a bare Log handler arm threads but is never invoked by emitted code. For the harness's capturing Log handler and Log audit entries to work, the demo must declare logging as a tool (Tool<Log>) in its fixtures.

**2026-07-28T06:18:33Z**

Three ticket-level clarifications resolved before implementation:

1. Logging is Tool<Log>, not a bare Log effect. ADR-022's consequence stands:
   bare-effect arms thread but are never invoked, so the demo declares a
   Tool<Log> with a structured argument and routes progress logging through
   it — interception and audit then apply uniformly. The acceptance-criterion
   line showing effects {Tool<ReadRepo>, Tool<CreateTicket>, Log} is stale;
   read it as {Tool<ReadRepo>, Tool<CreateTicket>, Tool<Log>}. The
   phrasebook's actor/handle examples still show bare Log arms that would
   silently never fire; updating them to Tool<Log> is in scope for this
   ticket.

2. The harness verifies via the audit stream, not a capturing handler.
   ADR-023 admits only pure install handlers, so the ticketed "capturing Log
   handler" is inexpressible in Hirð; unconditional dispatch recording
   (ADR-022 §2) makes it unnecessary. The harness is a Rust integration test
   in hird-cli/tests (erlc-gated, alongside the existing CLI tests): it runs
   `hird run` on a harness variant of the demo whose install block supplies
   the mock handlers, then asserts on the audit JSON lines from stdout —
   expected CreateTicket invocations (ticket creation) and expected
   ReadRepo/Log records. "Verifies tickets were created via mock" and
   "verifies the audit log" collapse into the same stdout assertions.

3. Shutdown is a no-op sentinel in v0.1. Actor handlers can only return new
   state — codegen has no gen_server stop path, and a permanent child would
   be restarted anyway. The demo's Shutdown arm returns state unchanged
   (exactly the phrasebook form), satisfying mailbox exhaustiveness and
   demonstrating the message-type surface. Real graceful termination (a
   stop-return in actor codegen plus restart-policy interplay) is new design
   surface on ADR-020 and out of scope for the demo; it can be ticketed
   separately if v0.2 wants it.

**2026-07-28T06:50:25Z**

Implemented and verified. demo/agent_planner.hird is the demo; the
harness and CLI coverage live in crates/hird-cli/tests/demo.rs; the
README documents build/run/emit-effect-graph; the phrasebook's bare Log
arms moved to Tool<Log> per clarification 1.

Two deviations from the ticket text, both forced by v0.1 as shipped:

1. The file is demo/agent_planner.hird, not demo/planner.hird. A
   planner.hird base module and the Planner actor both derive the Erlang
   module hird_planner, and `hird build` rejects the collision. The
   actor keeps the ticketed name (it is what the effect graph and the
   audit callers show); the file name absorbs the rename.

2. `hird run` drives a directly spawned Planner rather than one running
   under PlannerSup. v0.1 has no surface form that starts a supervisor:
   `spawn` resolves actor names only, and hir-y9jo explicitly rejected
   implicit start-all-supervisors. PlannerSup is fully real — checked,
   emitted as an OTP supervisor module (hird_planner_sup.erl, compiled
   by the build), and present in the effect graph with strategy and
   child — and supervised crash-restart was verified on BEAM at the
   runtime-library level (hir-7oph). A supervisor-start expression
   (plus typed child lookup, hird_sup_util:child_pid's consumer) is new
   design surface on ADR-018/020; ticket separately if v0.2 wants it,
   as with the Shutdown stop path.

Acceptance verified: check/build/run/emit-effect-graph all pass on the
demo (erlc-gated tests assert the artifacts, the audit JSON stream —
ReadRepo, two CreateTicket invocations for the actionable tasks, Log
records, both caller forms — and the graph's actor/supervisor/tool
content). The harness re-runs the same source with mock handlers
swapped into the install block and asserts expected mock tickets and
log records on the audit stream. cargo fmt/clippy/test pass across the
workspace.

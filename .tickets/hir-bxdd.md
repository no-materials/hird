---
id: hir-bxdd
status: open
deps: [hir-7oph, hir-y9jo, hir-1dvq, hir-z9rn]
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


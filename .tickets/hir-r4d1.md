---
id: hir-r4d1
status: closed
deps: [hir-ugi0]
links: []
created: 2026-07-28T07:59:04Z
type: task
priority: 0
assignee: nomaterials
parent: hir-xixm
tags: [supervision, demo]
---
# Run the demo planner under PlannerSup

Move demo/agent_planner.hird onto the supervisor runtime surface
(hir-ugi0) so `hird run` drives a planner that genuinely runs under
PlannerSup, closing the deviation recorded on hir-bxdd.

Scope:
- `run_demo` starts the tree with `supervise(PlannerSup)` and obtains
  the planner via `child(PlannerSup, planner)` instead of spawning it
  directly; `main`'s row swaps `Spawn<PlannerMsg>` for `Supervise`.
- The dry-run harness in crates/hird-cli/tests/demo.rs keeps passing
  unchanged in spirit (same audit assertions; the harness variant is
  derived from the demo source, so it follows automatically).
- README: update the demo walkthrough to name the supervised path.
- phrasebook.md: document `supervise` and `child` next to Typed
  References (spawn/send/request/reply).
- hir-bxdd note: append that deviation 2 is closed.

Out of scope: an in-program restart demonstration (see the epic's out of
scope — observation races the crash); the restart story remains "the
tree is real and OTP restarts the child", verified at the runtime-library
level.

## Acceptance Criteria

- `hird run demo/agent_planner.hird` produces the same seven-record
  audit stream as before, with the planner running as PlannerSup's
  child.
- `hird emit-effect-graph` output is unchanged apart from any row
  updates (`Supervise` in main is not part of the graph's actor or
  supervisor nodes).
- Demo integration tests and the harness pass; fmt, clippy -D warnings,
  and workspace tests pass.
- README and phrasebook updated.


## Notes

**2026-07-28T13:44:17Z**

Implemented in commit 828f5de (demo: run the planner under PlannerSup
via supervise and child). run_demo supervises PlannerSup and obtains
the planner via child(PlannerSup, planner); main's row swaps
Spawn<PlannerMsg> for Supervise; the unused Spawn effect declaration is
dropped.

Acceptance verified: hird run produces the same seven-record audit
stream (Log, ReadRepo, CreateTicket, Log, CreateTicket, Log, Log) with
the planner running as PlannerSup's supervised child; the
emit-effect-graph output diff against the pre-change demo is line
numbers only (the graph has no row content from main); all five demo
integration tests including the dry-run harness pass; README and
phrasebook updated; hir-bxdd notes deviation 2 closed; fmt, clippy -D
warnings, and the full workspace suite pass.

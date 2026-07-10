---
id: hir-z9rn
status: closed
deps: [hir-zp13]
links: [hir-0bhk, hir-xbs5]
created: 2026-07-09T12:58:31Z
type: task
priority: 1
assignee: nomaterials
parent: hir-7rsf
tags: [phase-9, codegen, supervision, erlang]
---
# Supervisor codegen to Erlang

Lower supervisor declarations (`IrSupervisorDef`/`IrChildSpec`, produced by
hir-xbs5) to Erlang OTP `supervisor` behaviour modules. The error-vs-crash
boundary and the `crash!` primitive are frontend work (hir-0bhk); this ticket
is purely the backend slice.

Depends on hir-zp13: a child's `start_args` is an arbitrary pure `IrExpr`
(checked against the actor's sole init parameter), so rendering it reuses the
general expression emitter, variable-renaming, and ADT→tagged-tuple rules
rather than a bespoke mini-emitter.

**Supervisor codegen**:
```erlang
-module(planner_sup).
-behaviour(supervisor).
-export([start_link/0, init/1]).

start_link() ->
    supervisor:start_link({local, ?MODULE}, ?MODULE, []).

init([]) ->
    SupFlags = #{
        strategy => one_for_one,
        intensity => 5,
        period => 60
    },
    ChildSpecs = [
        #{
            id => planner,
            start => {planner, start_link, [default_config()]},
            restart => permanent,
            type => worker
        }
    ],
    {ok, {SupFlags, ChildSpecs}}.
```

## Acceptance Criteria

- Supervisor codegen produces valid Erlang supervisor behaviour modules.
- Generated modules have correct SupFlags (strategy, intensity, period) and
  ChildSpecs (id, start MFA, restart disposition, type).
- `one_for_one` is the only strategy lowered in v0.1 (per the epic's scope).
- `start_args` renders via the hir-zp13 expression emitter.
- Generated `.erl` compiles with stock `erlc`.
- Snapshot tests: single-child supervisor, supervisor with multiple children,
  each restart disposition (permanent/temporary/transient).
- At least 4 snapshot tests.

## Decisions locked (v0.1)

**Non-`one_for_one` strategies emit verbatim.** hir-xbs5 made `one_for_all`
and `rest_for_one` a frontend *warning*, not an error, so they reach the IR.
All three strategy names are valid OTP supervisor atoms and the emission code
is identical, so codegen renders `strategy` verbatim rather than skipping or
erroring. The "only one_for_one is lowered" scope line means only one_for_one
is supported/tested; the others compile but carry the frontend's
not-yet-implemented warning.

**Module naming follows the locked `hird_` convention, not the sketch.**
`PlannerSup` emits as module `hird_planner_sup` (via `erlang_module_name`),
consistent with actor and base modules. The `planner_sup` in the sketch above
is illustrative only.

**Child specs omit the `shutdown` key.** OTP's default (5000 for workers)
applies. Likewise `type => worker` is written explicitly since all v0.1
children are actors.

**Supervisor registers as `{local, Module}`; children stay unregistered.**
Matches the sketch and hir-1dvq's unregistered `gen_server:start_link/3`
mapping. How the demo reaches a child's pid (`supervisor:which_children/1`
or runtime-library support) is hir-7oph/hir-bxdd's concern, not this
ticket's.


## Notes

**2026-07-10T13:37:34Z**

Implemented and closed. hird-codegen emits one supervisor behaviour module per supervisor declaration: start_link/0 registering {local, Module}, init/1 with SupFlags (strategy rendered verbatim per the locked decision, intensity, period) and one child-spec map per child (id, {actor_module, start_link, [start_args]}, restart disposition, explicit worker type, shutdown left to the OTP default; children unregistered). start_args renders through the hir-zp13 expression emitter with one variable scope spanning all children, so let-bindings across children freshen instead of colliding in init/1's shared Erlang scope. 5 snapshot tests (single child, multi-child covering permanent/transient/temporary, let-bound start_args, verbatim one_for_all, empty children), all erlc-validated. Also verified end to end on BEAM: supervisor starts, which_children reports the worker started from the rendered start_args, and a killed child is restarted one_for_one with fresh init state. Commit: 085c2c1.

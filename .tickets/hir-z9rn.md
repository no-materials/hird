---
id: hir-z9rn
status: open
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


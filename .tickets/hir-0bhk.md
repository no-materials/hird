---
id: hir-0bhk
status: open
deps: [hir-xbs5]
links: [hir-fbze]
created: 2026-05-22T21:40:54Z
type: task
priority: 1
assignee: nomaterials
parent: hir-cnq8
tags: [phase-8, supervision, errors, codegen]
---
# Error-vs-crash boundary and supervisor codegen

Implement the error-vs-crash boundary and lower supervisors to Erlang OTP
supervisor behavior modules.

**Error-vs-crash boundary** (resolves OD1):

Domain errors are values in effect rows:
- `Exn ParseError` — a domain error carried as an effect.
- Handled with pattern matching or effect handlers.
- Does NOT kill the process.
- Example: parsing tool output fails → Exn ParseError → caller decides what to do.

Crashes are resource failures that reach the supervisor:
- `crash!("message")` or `panic!("message")` — explicit process termination.
- Runtime failures (out of memory, network disconnect) also crash.
- Propagate as Erlang exits; caught by supervisor for restart.
- Example: network timeout during tool call → crash → supervisor restarts actor.

The language enforces:
- A function with only Exn effects cannot crash (barring bugs/OOM).
- A function that calls crash! has that visible in its signature or call context
  (design: crash! is a divergent function that never returns, like Rust's panic!).
- The compiler can warn if a function catches ALL Exn errors but still might crash
  due to calling a function that uses crash!.

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

**Documentation**: docs/error-model.md explaining the boundary, when to use each,
and how they interact with effect rows and supervision.

## Acceptance Criteria

- crash!/panic! primitive exists; it is a divergent expression (never returns).
- Domain errors (Exn effects) do not kill the process.
- Crashes propagate as Erlang exits to supervisor.
- Supervisor codegen produces valid Erlang supervisor behavior modules.
- Generated supervisor modules have correct SupFlags and ChildSpecs.
- docs/error-model.md written with examples and clear guidance.
- OD1 documented in DECISIONS.md.
- Snapshot tests: crash! codegen, supervisor codegen, error-vs-crash distinction
  in types, supervisor with multiple children.
- At least 6 snapshot tests.


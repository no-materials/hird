---
id: hir-1dvq
status: open
deps: [hir-m6ra, hir-zp13, ha-8fyg]
links: []
created: 2026-05-22T21:40:27Z
type: task
priority: 1
assignee: nomaterials
parent: hir-7rsf
tags: [phase-9, actors, codegen]
---
# Actor codegen to Erlang gen_server

Lower actor declarations to Erlang gen_server behavior modules.

**Generated Erlang structure** for an actor like Planner:
```erlang
-module(planner).
-behaviour(gen_server).
-export([start_link/1, plan_repo/2, get_status/1, shutdown/1]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2]).

start_link(Config) ->
    gen_server:start_link(?MODULE, Config, []).

plan_repo(Pid, Path) ->
    gen_server:cast(Pid, {plan_repo, Path}).

get_status(Pid) ->
    gen_server:call(Pid, get_status).

shutdown(Pid) ->
    gen_server:cast(Pid, shutdown).

init(Config) ->
    %% calls the Hirð init function
    {ok, hird_planner_init(Config)}.

handle_call(get_status, From, State) ->
    %% calls the Hirð GetStatus handler
    {reply, hird_planner_get_status(State), State}.

handle_cast({plan_repo, Path}, State) ->
    %% calls the Hirð PlanRepo handler
    NewState = hird_planner_plan_repo(Path, State),
    {noreply, NewState}.

handle_cast(shutdown, State) ->
    {stop, normal, State}.
```

**Mapping**: implement the actor-to-Erlang mapping locked by the Phase 7
design ADR (ha-8fyg): ReplyTo representation, call/cast dispatch rule,
request argument restriction, module layout and naming. The sketch above
is illustrative; the ADR is authoritative.

**Baseline mapping** (from the epic design context):
- Actor state is the gen_server state.
- init/1 calls the Hirð init function.
- Handler return type determines gen_server reply tuple shape.
- Pid<Msg> is a regular Erlang pid at runtime; type safety is
  compile-time only.
- spawn maps to gen_server:start_link.

Depends on the expression emitter (hir-zp13): handler bodies are
arbitrary IR expressions, and spawn/send/request/reply lower inside
ordinary function bodies. This ticket produces gen_server modules
validated with erlc but does NOT test them on BEAM — that's the
end-to-end demo ticket.

## Acceptance Criteria

- Actor declarations lower to gen_server Erlang modules.
- Generated modules have correct -behaviour(gen_server) and exports.
- ReplyTo messages map to handle_call; fire-and-forget to handle_cast.
- spawn lowers to gen_server:start_link.
- send lowers to gen_server:cast; request to gen_server:call;
  reply to gen_server:reply.
- Generated Erlang is human-readable and compiles with stock erlc.
- Snapshot tests: generated Erlang for a simple actor, for an actor with
  ReplyTo messages, for spawn/send/request/reply call sites.
- At least 5 snapshot tests of generated Erlang.


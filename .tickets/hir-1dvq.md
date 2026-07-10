---
id: hir-1dvq
status: closed
deps: [hir-m6ra, hir-zp13, ha-8fyg, hc-uz3r]
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

**Mapping**: implement the actor-to-Erlang mapping locked by ADR-020
(ha-8fyg): ReplyTo-as-From, per-constructor call/cast dispatch, bare
constructors in request, explicit gen_server:reply with {noreply, State},
hird_-prefixed module naming. The sketch above is illustrative; the ADR
is authoritative.

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


## Notes

**2026-07-10T09:17:20Z**

Scope boundary per ADR-022 discussion: the spawn/send/request/reply *expressions* are emitted by hir-zp13's expression emitter (they lower inside ordinary function bodies); this ticket emits the gen_server behaviour-module shells (init/1, handle_call/3, handle_cast/2, exports) around handler bodies, reusing that emitter.

**2026-07-10T12:10:01Z**

Pre-implementation decisions locked:

1. **Handler maps never cross the spawn boundary** (ADR-020 §6, amended
   2026-07-10). Generated callbacks invoke init and handler bodies with
   `#{}`; tool calls inside actors resolve via ADR-022 §3's registry
   fallback. Rationale in the ADR (supervisor restarts have no spawner
   context; a snapshot would exceed what effect rows state; forbid-then-
   relax is additive). Shared contract with hir-7oph and hir-bxdd's test
   harness: mocks for spawned actors install via the registry.

2. **No per-message client wrapper functions.** The ticket sketch's
   plan_repo/2, get_status/1 etc. would be dead code — call sites emit
   gen_server:cast/call directly (hir-zp13). Only start_link is emitted
   (needed by spawn and by supervisor child specs).

3. **No shutdown/stop clause.** The sketch's shutdown → {stop, normal}
   has no surface primitive behind it; v0.1 actors run until crash or
   supervisor shutdown.

4. **Only required callbacks emitted**: init/1, handle_call/3,
   handle_cast/2. handle_info/terminate/code_change are optional
   gen_server callbacks in modern OTP; rely on defaults, keep modules
   minimal and readable.

5. **Codegen public API grows multi-module output** — one .erl per actor
   (ADR-020 §5) plus the base module, returned as named (module, source)
   pairs instead of a single string. Affects hir-y9jo's `hird build`.

Reminder from the sketch-vs-ADR check: handle_call clauses always return
{noreply, State} with explicit gen_server:reply (ADR-020 §4); call
payloads are bare constructor atoms, reply_to binds From.

**2026-07-10T12:27:55Z**

Implemented in hird-codegen (commit 3d70a20). Each actor emits a
gen_server behaviour module: start_link at init arity (multi-param
inits pack into the single init/1 argument), init/1 wraps the Hirð
init body in {ok, State}, call constructors (ReplyTo field) become
handle_call clauses with bare-atom payloads / From-bound reply channel
/ explicit gen_server:reply / {noreply, NextState}, cast constructors
become handle_cast clauses on their ADT wire shape. Callbacks run
bodies against no handler map (registry fallback); a side with no
constructors gets a crashing fallback clause so all three required
callbacks exist. Actor modules qualify base-module function references.
Public API is now emit_modules → Vec<EmittedModule> (base module first,
then one per actor) — hir-y9jo's build command consumes the pairs.
Verified: 6 new actor snapshots (24 codegen tests total), erlc compiles
every emitted module of every fixture, and a Planner-shaped end-to-end
drive (parse → check → lower → emit → erlc) produced a working
hird_planner gen_server module.

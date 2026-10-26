%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Supervised-child scenarios: `child(Sup, id)` lowered as generated code
%% lowers it, through hird_sup_util:child_pid/2 on the supervisor's
%% registered name. The module is its own supervisor, registered under
%% its name as generated supervisors are, over two children: `steady`,
%% the lookup target, and `slow`, a sibling whose restarts take the
%% scenario's init time.
-module(hird_bench_sup).
-behaviour(supervisor).

-export([child/1, restart/1]).
-export([init/1, start_child/2]).

%% `child`: child lookup and a send, by caller count.
-spec child(boolean()) -> ok.
child(Quick) ->
    hird_bench:title(
        "child",
        "N child(Sup, steady) lookups each followed by a send, split over "
        "callers",
        ["callers", "ops/s", "p50", "p99", "max", "memory"]),
    Total = case Quick of
        true -> 5000;
        false -> 200000
    end,
    _ = [child_case(Callers, Total) || Callers <- [1, 16]],
    ok.

%% One child row.
child_case(Callers, Total) ->
    Sup = start_sup(0),
    Hist = hird_bench:hist(),
    Share = Total div Callers,
    Sampler = hird_bench:sampler(fun() -> hird_bench:probe(?MODULE) end, 10),
    {Start, Done} = hird_bench:parallel(
        lists:duplicate(Callers, fun() -> send(Share, Hist) end)),
    {_, Memory} = hird_bench:peaks(hird_bench:stop(Sampler)),
    ok = stop_sup(Sup),
    hird_bench:row(
        [hird_bench:int(Callers),
         hird_bench:rate(Share * Callers / hird_bench:seconds(Done - Start))]
        ++ hird_bench:latency(Hist) ++ [hird_bench:mib(Memory)]).

%% `N` lookups of `steady`, each followed by a cast to it; sampled ones
%% timed into `Hist`.
send(0, _Hist) ->
    ok;
send(N, Hist) ->
    ok = hird_bench:timed(N, Hist, fun() ->
        gen_server:cast(lookup(steady), ping)
    end),
    send(N - 1, Hist).

%% `restart`: lookups of `steady` while `slow` restarts.
-spec restart(boolean()) -> ok.
restart(Quick) ->
    InitMs = case Quick of
        true -> 100;
        false -> 1000
    end,
    Margin = InitMs div 5,
    hird_bench:title(
        "restart",
        lists:flatten(io_lib:format(
            "callers look up steady back to back for ~b ms, then its "
            "sibling is killed and restarts with a ~b ms init; steady rows "
            "are the lookups begun before the kill, restarting rows those "
            "begun during the init", [Margin, InitMs])),
        ["case", "ops/s", "p50", "p99", "max", "lookups", "memory"]),
    _ = [restart_case(Callers, InitMs, Margin) || Callers <- [1, 16]],
    ok.

%% The steady and restarting rows for one caller count.
restart_case(Callers, InitMs, Margin) ->
    Sup = start_sup(InitMs),
    Slow = lookup(slow),
    Steady = hird_bench:hist(),
    Restarting = hird_bench:hist(),
    %% The kill's time, as native units after `Ref` (0 until it happens).
    Killed = atomics:new(1, []),
    Ref = erlang:monotonic_time(),
    End = Ref + hird_bench:native(Margin + InitMs),
    Killer = fun() ->
        timer:sleep(Margin),
        atomics:put(Killed, 1, erlang:monotonic_time() - Ref),
        exit(Slow, kill)
    end,
    Caller = fun() -> look_up(Ref, Killed, End, Steady, Restarting) end,
    Sampler = hird_bench:sampler(fun() -> hird_bench:probe(?MODULE) end, 10),
    {Start, _} = hird_bench:parallel(
        [Killer | lists:duplicate(Callers, Caller)]),
    {_, Memory} = hird_bench:peaks(hird_bench:stop(Sampler)),
    ok = stop_sup(Sup),
    Kill = Ref + atomics:get(Killed, 1),
    Label = case Callers of
        1 -> "1 caller";
        _ -> integer_to_list(Callers) ++ " callers"
    end,
    restart_row(Label ++ ", steady", Steady, Kill - Start, Memory),
    restart_row(Label ++ ", restarting", Restarting, End - Kill, Memory).

%% One restart row over a window `Native` long.
restart_row(Label, Hist, Native, Memory) ->
    N = hird_bench:count(Hist),
    hird_bench:row(
        [Label, hird_bench:rate(N / hird_bench:seconds(Native))]
        ++ hird_bench:latency(Hist)
        ++ [hird_bench:int(N), hird_bench:mib(Memory)]).

%% Timed lookups of `steady` begun before `End`: those begun once the
%% kill happened into `Restarting`, earlier ones into `Steady`.
look_up(Ref, Killed, End, Steady, Restarting) ->
    T0 = erlang:monotonic_time(),
    case T0 >= End of
        true ->
            ok;
        false ->
            _ = lookup(steady),
            T1 = erlang:monotonic_time(),
            Kill = atomics:get(Killed, 1),
            Hist = case Kill > 0 andalso T0 - Ref >= Kill of
                true -> Restarting;
                false -> Steady
            end,
            hird_bench:record(Hist, T1 - T0),
            look_up(Ref, Killed, End, Steady, Restarting)
    end.

%% `child(hird_bench_sup, Id)` as generated code emits it.
lookup(Id) ->
    case hird_sup_util:child_pid(?MODULE, Id) of
        {ok, Pid} -> Pid;
        error -> erlang:error({no_child, Id})
    end.

%% Starts the supervisor; `slow` restarts take `InitMs`.
start_sup(InitMs) ->
    {ok, Sup} = supervisor:start_link({local, ?MODULE}, ?MODULE, InitMs),
    Sup.

%% Stops the supervisor and its children.
stop_sup(Sup) ->
    gen_server:stop(Sup).

%% @private
init(InitMs) ->
    Flags = #{strategy => one_for_one, intensity => 10, period => 10},
    {ok, {Flags, [spec(steady, 0), spec(slow, InitMs)]}}.

%% A child spec whose restarts take `InitMs`.
spec(Id, InitMs) ->
    #{id => Id,
      start => {?MODULE, start_child, [counters:new(1, []), InitMs]},
      restart => permanent,
      shutdown => brutal_kill,
      type => worker}.

%% @private A child that drains its mailbox. Every start after the first
%% sleeps `InitMs` first, holding the supervisor in the start call as a
%% restarting actor's slow init does.
start_child(Starts, InitMs) ->
    case counters:get(Starts, 1) of
        0 -> ok;
        _ -> timer:sleep(InitMs)
    end,
    counters:add(Starts, 1, 1),
    {ok, spawn_link(fun drain/0)}.

%% Discards every message.
drain() ->
    receive _ -> drain() end.

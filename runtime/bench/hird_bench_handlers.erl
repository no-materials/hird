%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Handler-install scenario: hird_handlers:install_handler/2 replacing an
%% installed handler, against the number of processes on the node. The
%% registry is persistent_term, where a replace makes the node scan every
%% process for references to the old value.
-module(hird_bench_handlers).

-export([install/1]).

%% `install`: replace cost by process count.
-spec install(boolean()) -> ok.
install(Quick) ->
    {Counts, Reps} = case Quick of
        true -> {[0, 1000], 5};
        false -> {[0, 1000, 10000, 100000], 50}
    end,
    hird_bench:title(
        "install",
        lists:flatten(io_lib:format(
            "~b install_handler/2 calls replacing an installed handler, with "
            "N idle processes each holding a small state; the caller's time, "
            "memory the processes' node memory", [Reps])),
        ["processes", "ops/s", "p50", "p99", "max", "memory"]),
    _ = [install_case(Count, Reps) || Count <- Counts],
    ok.

%% One install row.
install_case(Count, Reps) ->
    Before = erlang:memory(total),
    Pids = [spawn_link(fun idle/0) || _ <- lists:seq(1, Count)],
    Memory = erlang:memory(total) - Before,
    Hist = hird_bench:hist(),
    Key = {tool, bench},
    {Start, Done} = hird_handlers:with_handlers([{Key, handler(0)}], fun() ->
        T0 = erlang:monotonic_time(),
        lists:foreach(fun(I) -> replace(Key, I, Hist) end,
                      lists:seq(1, Reps)),
        {T0, erlang:monotonic_time()}
    end),
    _ = [begin unlink(Pid), exit(Pid, kill) end || Pid <- Pids],
    hird_bench:row(
        [hird_bench:int(Count),
         hird_bench:rate(Reps / hird_bench:seconds(Done - Start))]
        ++ hird_bench:latency(Hist) ++ [hird_bench:mib(Memory)]).

%% One timed replace of the handler under `Key` with handler `I`.
replace(Key, I, Hist) ->
    Handler = handler(I),
    T0 = erlang:monotonic_time(),
    ok = hird_handlers:install_handler(Key, Handler),
    hird_bench:record(Hist, erlang:monotonic_time() - T0).

%% A handler answering `I`: a distinct value per `I`, so each install
%% replaces rather than repeats.
handler(I) ->
    fun(_Args, _Handlers) -> I end.

%% An idle process holding a 100-element list.
idle() ->
    State = lists:seq(1, 100),
    receive stop -> length(State) end.

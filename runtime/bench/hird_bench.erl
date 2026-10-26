%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% The runtime benchmark driver: runs one scenario (or all of them) and
%% prints a table per scenario on standard_error, which bench.sh turns
%% into its standard output. Also the scenarios' shared kit: a latency
%% histogram many processes record into, a memory sampler, releasing
%% worker processes together, and table formatting.
-module(hird_bench).

-export([main/1]).
-export([hist/0, record/2, timed/3, latency/1, count/1, parallel/1, seconds/1,
         native/1, sampler/2, stop/1, probe/1, peaks/1,
         title/3, row/1, rate/1, nanos/1, mib/1, int/1]).

%% One operation in SAMPLE_MASK + 1 is timed in a throughput loop.
-define(SAMPLE_MASK, 15).

%% Log-linear histogram buckets: 2^SUB_BITS per power of two, so a
%% value under 2^(SUB_BITS + 1) is exact and any other is at most 1/32
%% off (half that at the bucket midpoint reported).
-define(SUB_BITS, 5).
-define(BUCKETS, (64 bsl ?SUB_BITS)).

%% Runs the scenario the arguments name, then halts: 0 when it completed,
%% 1 when it crashed, 2 on a usage error.
-spec main([string()]) -> no_return().
main(Args) ->
    Status = case parse(Args, false) of
        {ok, Runs, Quick} -> run(Runs, Quick);
        usage -> usage(), 2
    end,
    halt(Status).

%% Every scenario, in the order `all` runs them.
scenarios() ->
    [{"audit", fun hird_bench_audit:audit/1},
     {"sync", fun hird_bench_audit:sync/1},
     {"mailbox", fun hird_bench_audit:mailbox/1},
     {"replay", fun hird_bench_replay:replay/1},
     {"child", fun hird_bench_sup:child/1},
     {"restart", fun hird_bench_sup:restart/1},
     {"install", fun hird_bench_handlers:install/1}].

%% `<scenario> [--quick]`, `--quick` in any position.
parse(["--quick" | Rest], _Quick) ->
    parse(Rest, true);
parse(["all" | Rest], Quick) ->
    flags(scenarios(), Rest, Quick);
parse([Name | Rest], Quick) ->
    case lists:keyfind(Name, 1, scenarios()) of
        false -> usage;
        Scenario -> flags([Scenario], Rest, Quick)
    end;
parse([], _Quick) ->
    usage.

%% The flags after the scenario name.
flags(Runs, [], Quick) -> {ok, Runs, Quick};
flags(Runs, ["--quick" | Rest], _Quick) -> flags(Runs, Rest, true);
flags(_Runs, _Other, _Quick) -> usage.

%% Prints the command line and the scenario names.
usage() ->
    Names = lists:join(" | ", [N || {N, _} <- scenarios()] ++ ["all"]),
    io:format(standard_error,
              "usage: bench.sh <scenario> [--quick]~n  scenario: ~s~n",
              [Names]).

%% Runs the scenarios in a monitored process, so a crash anywhere in
%% them, a linked runtime process's included, ends the run with status 1
%% rather than a hung node.
run(Runs, Quick) ->
    io:format(standard_error,
              "hird runtime bench: OTP ~s, erts ~s, ~b schedulers~s~n"
              "latencies include ~s of clock reads; throughput loops time "
              "one operation in ~b~n",
              [erlang:system_info(otp_release), erlang:system_info(version),
               erlang:system_info(schedulers_online),
               case Quick of true -> ", quick sizes"; false -> "" end,
               nanos(clock_overhead()), ?SAMPLE_MASK + 1]),
    {Pid, Ref} = spawn_monitor(fun() -> [Fun(Quick) || {_, Fun} <- Runs] end),
    receive
        {'DOWN', Ref, process, Pid, normal} ->
            0;
        {'DOWN', Ref, process, Pid, Reason} ->
            io:format(standard_error, "hird_bench: ~p~n", [Reason]),
            1
    end.

%% Latency histograms -----------------------------------------------------

%% A latency histogram over native time units; any number of processes
%% record into it concurrently.
-spec hist() -> counters:counters_ref().
hist() ->
    counters:new(?BUCKETS, [write_concurrency]).

%% Records one duration, in native time units.
-spec record(counters:counters_ref(), non_neg_integer()) -> ok.
record(Hist, Native) ->
    counters:add(Hist, bucket(Native) + 1, 1).

%% Runs `Fun`, the `N`th operation of a throughput loop, timing it into
%% `Hist` when `N` is sampled: a clock read is a syscall on some clock
%% sources (HPET), slow enough to bound a loop that times every call.
-spec timed(non_neg_integer(), counters:counters_ref(), fun(() -> R)) -> R.
timed(N, Hist, Fun) when N band ?SAMPLE_MASK =:= 0 ->
    T0 = erlang:monotonic_time(),
    Result = Fun(),
    record(Hist, erlang:monotonic_time() - T0),
    Result;
timed(_N, _Hist, Fun) ->
    Fun().

%% The median back-to-back clock read pair in nanoseconds: what timing
%% adds to every latency.
clock_overhead() ->
    Pairs = lists:sort([begin
                            T0 = erlang:monotonic_time(),
                            erlang:monotonic_time() - T0
                        end || _ <- lists:seq(1, 1001)]),
    erlang:convert_time_unit(lists:nth(501, Pairs), native, nanosecond).

%% A value's 0-based bucket.
bucket(N) when N < (2 bsl ?SUB_BITS) -> N;
bucket(N) -> bucket(N bsr 1, 1).

%% The bucket of a value shifted right `E` times so far.
bucket(M, E) when M < (2 bsl ?SUB_BITS) ->
    min(((E + 1) bsl ?SUB_BITS) + M - (1 bsl ?SUB_BITS), ?BUCKETS - 1);
bucket(M, E) ->
    bucket(M bsr 1, E + 1).

%% The midpoint of a 0-based bucket, in native time units.
midpoint(I) when I < (2 bsl ?SUB_BITS) ->
    I;
midpoint(I) ->
    E = (I bsr ?SUB_BITS) - 1,
    Low = ((I band ((1 bsl ?SUB_BITS) - 1)) + (1 bsl ?SUB_BITS)) bsl E,
    Low + ((1 bsl E) bsr 1).

%% The p50, p99 and max cells; `-` each when nothing was recorded.
-spec latency(counters:counters_ref()) -> [string()].
latency(Hist) ->
    Counts = buckets(Hist),
    case lists:sum(Counts) of
        0 -> ["-", "-", "-"];
        Total -> [nanos(percentile(P, Counts, Total)) || P <- [50, 99, 100]]
    end.

%% How many durations were recorded.
-spec count(counters:counters_ref()) -> non_neg_integer().
count(Hist) ->
    lists:sum(buckets(Hist)).

%% Every bucket's count, lowest first.
buckets(Hist) ->
    [counters:get(Hist, I) || I <- lists:seq(1, ?BUCKETS)].

%% The nearest-rank percentile `P` of a non-empty histogram, in
%% nanoseconds.
percentile(P, Counts, Total) ->
    Rank = max(1, ceil(P * Total / 100)),
    Native = midpoint(rank_bucket(Rank, Counts, 0, 0)),
    erlang:convert_time_unit(Native, native, nanosecond).

%% The 0-based bucket holding the `Rank`th smallest value.
rank_bucket(Rank, [C | Rest], I, Seen) ->
    case Seen + C >= Rank of
        true -> I;
        false -> rank_bucket(Rank, Rest, I + 1, Seen + C)
    end.

%% Timing ------------------------------------------------------------------

%% Runs every fun in its own linked process, all released together;
%% the monotonic times at release and once the last one returned.
-spec parallel([fun(() -> term())]) -> {integer(), integer()}.
parallel(Funs) ->
    Self = self(),
    Pids = [spawn_link(fun() ->
                receive go -> ok end,
                _ = Fun(),
                Self ! {done, self()}
            end) || Fun <- Funs],
    Start = erlang:monotonic_time(),
    [Pid ! go || Pid <- Pids],
    [receive {done, Pid} -> ok end || Pid <- Pids],
    {Start, erlang:monotonic_time()}.

%% A native monotonic-time difference in seconds.
-spec seconds(integer()) -> float().
seconds(Native) ->
    Native / erlang:convert_time_unit(1, second, native).

%% Milliseconds in native time units.
-spec native(non_neg_integer()) -> integer().
native(Ms) ->
    erlang:convert_time_unit(Ms, millisecond, native).

%% Memory sampling ---------------------------------------------------------

%% Starts calling `Probe` every `IntervalMs`, once right away; stop/1
%% ends it and returns the samples, oldest first, as
%% `{ElapsedMs, Probe()}`.
-spec sampler(fun(() -> map()), pos_integer()) -> pid().
sampler(Probe, IntervalMs) ->
    T0 = erlang:monotonic_time(millisecond),
    spawn_link(fun() -> sample(Probe, IntervalMs, T0, []) end).

%% The sampler loop.
sample(Probe, IntervalMs, T0, Acc) ->
    Samples = [{erlang:monotonic_time(millisecond) - T0, Probe()} | Acc],
    receive
        {stop, From} -> From ! {samples, self(), lists:reverse(Samples)}
    after IntervalMs ->
        sample(Probe, IntervalMs, T0, Samples)
    end.

%% Stops a sampler, with a final sample; its samples.
-spec stop(pid()) -> [{integer(), map()}].
stop(Sampler) ->
    Sampler ! {stop, self()},
    receive {samples, Sampler, Samples} -> Samples end.

%% Node memory, and the message queue and memory of `Target` (a pid or
%% registered name; zeros when it is not running).
-spec probe(pid() | atom()) ->
    #{node := non_neg_integer(), queue := non_neg_integer(),
      process := non_neg_integer()}.
probe(Target) ->
    Pid = case Target of
        Name when is_atom(Name) -> whereis(Name);
        _ -> Target
    end,
    Info = case is_pid(Pid) andalso
                process_info(Pid, [message_queue_len, memory]) of
        [{message_queue_len, Q}, {memory, M}] -> #{queue => Q, process => M};
        _ -> #{queue => 0, process => 0}
    end,
    Info#{node => erlang:memory(total)}.

%% Peak queue length, and peak node memory growth over the first sample.
-spec peaks([{integer(), map()}]) -> {non_neg_integer(), integer()}.
peaks([{_, #{node := Base}} | _] = Samples) ->
    {lists:max([Q || {_, #{queue := Q}} <- Samples]),
     lists:max([N || {_, #{node := N}} <- Samples]) - Base}.

%% Tables ------------------------------------------------------------------

%% Prints a scenario's name and what it measures, then its column header.
-spec title(string(), string(), [string()]) -> ok.
title(Name, Note, Columns) ->
    io:format(standard_error, "~n~s: ~s~n", [Name, Note]),
    row(Columns).

%% Prints one table row: a label column, then right-aligned cells.
-spec row([iodata()]) -> ok.
row([Label | Cells]) ->
    io:format(standard_error, "~-22s~s~n",
              [Label, [io_lib:format("~11s", [C]) || C <- Cells]]).

%% An operation rate per second.
-spec rate(number()) -> string().
rate(PerSec) when PerSec >= 1.0e6 -> fmt("~.2fM", [PerSec / 1.0e6]);
rate(PerSec) when PerSec >= 1.0e3 -> fmt("~.1fk", [PerSec / 1.0e3]);
rate(PerSec) -> fmt("~.1f", [float(PerSec)]).

%% A duration in nanoseconds.
-spec nanos(non_neg_integer()) -> string().
nanos(Ns) when Ns >= 1000000000 -> fmt("~.2fs", [Ns / 1.0e9]);
nanos(Ns) when Ns >= 1000000 -> fmt("~.2fms", [Ns / 1.0e6]);
nanos(Ns) when Ns >= 1000 -> fmt("~.2fus", [Ns / 1.0e3]);
nanos(Ns) -> fmt("~bns", [Ns]).

%% A byte count in MiB.
-spec mib(integer()) -> string().
mib(Bytes) -> fmt("~.1fMiB", [Bytes / 1048576]).

%% An integer.
-spec int(integer()) -> string().
int(N) -> integer_to_list(N).

%% Formats to a flat string.
fmt(Format, Args) -> lists:flatten(io_lib:format(Format, Args)).

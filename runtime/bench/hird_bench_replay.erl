%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Replay scenario: start a cursor over an N-record audit log, then offer
%% every record in order as `main`. Logs are written once per size under
%% _build and reused; remove them to regenerate.
-module(hird_bench_replay).

-export([replay/1]).

%% `replay`: load time and memory by log length, then offer throughput.
-spec replay(boolean()) -> ok.
replay(Quick) ->
    hird_bench:title(
        "replay",
        "hird_replay:start_link/2 over an N-record log, then every record "
        "offered in order; cursor is its memory after load and a GC, memory "
        "the node's peak growth during load",
        ["records", "load", "log", "cursor", "memory", "offers/s", "p50",
         "p99", "max"]),
    Sizes = case Quick of
        true -> [5000];
        false -> [50000, 200000, 1000000]
    end,
    _ = [replay_case(N) || N <- Sizes],
    ok.

%% One replay row; the cursor is stopped before the next size loads.
replay_case(N) ->
    Path = log(N),
    Sampler = hird_bench:sampler(
        fun() -> hird_bench:probe(hird_replay) end, 50),
    T0 = erlang:monotonic_time(),
    {ok, Cursor} = hird_replay:start_link(Path, [table()]),
    Loaded = erlang:monotonic_time(),
    {_, Memory} = hird_bench:peaks(hird_bench:stop(Sampler)),
    true = erlang:garbage_collect(Cursor),
    {memory, Held} = process_info(Cursor, memory),
    Hist = hird_bench:hist(),
    {Start, Done} = hird_bench:parallel([fun() -> offer(1, N, Hist) end]),
    ok = hird_replay:finish(),
    ok = gen_server:stop(Cursor),
    hird_bench:row(
        [hird_bench:int(N),
         hird_bench:nanos(erlang:convert_time_unit(Loaded - T0, native,
                                                   nanosecond)),
         hird_bench:mib(filelib:file_size(Path)),
         hird_bench:mib(Held), hird_bench:mib(Memory),
         hird_bench:rate(N / hird_bench:seconds(Done - Start))]
        ++ hird_bench:latency(Hist)).

%% Offers records `Seq` to `N` in order, sampled offers timed into `Hist`.
offer(Seq, N, _Hist) when Seq > N ->
    ok;
offer(Seq, N, Hist) ->
    Args = args(Seq),
    {ok, Seq} = hird_bench:timed(Seq, Hist, fun() ->
        hird_replay:offer(<<"main">>, echo, [], Args)
    end),
    offer(Seq + 1, N, Hist).

%% The path of the `N`-record log, written first when missing.
log(N) ->
    Path = "_build/replay_main_" ++ integer_to_list(N) ++ ".jsonl",
    case filelib:is_regular(Path) of
        true ->
            Path;
        false ->
            Partial = Path ++ ".partial",
            {ok, Fd} = file:open(Partial, [write, raw, binary, delayed_write]),
            ok = write_lines(Fd, 1, N),
            ok = file:close(Fd),
            ok = file:rename(Partial, Path),
            Path
    end.

%% Writes the lines for records `Seq` to `N`.
write_lines(_Fd, Seq, N) when Seq > N ->
    ok;
write_lines(Fd, Seq, N) ->
    Line = hird_types:encode_invocation(
        #{tool => echo, type_args => [], args => args(Seq),
          result => {ok, Seq}, timestamp => Seq,
          caller => <<"Bench.main@main">>},
        table()),
    ok = file:write(Fd, [Line, $\n]),
    write_lines(Fd, Seq + 1, N).

%% Record `Seq`'s args: 16 ASCII digits.
args(Seq) ->
    iolist_to_binary(io_lib:format("~16..0b", [Seq])).

%% `Echo : String → Int`.
table() ->
    #{tools => #{echo => #{name => <<"Echo">>, params => 0, args => string,
                           result => int, error => dynamic}},
      types => #{}}.

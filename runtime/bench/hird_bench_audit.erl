%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Audit-path scenarios. Producers call hird_tool_dispatch:call/5 the way
%% generated code does, with a mock handler in the threaded map, so each
%% figure covers dispatch, record capture and the sink, wherever the
%% runtime does the encoding. Each call's args are a fresh copy of the
%% payload, so a backlog holds its payloads as a real program's would.
-module(hird_bench_audit).

-export([audit/1, sync/1, mailbox/1]).

%% The file sink's log, under bench.sh's working directory.
-define(LOG, "_build/bench_audit.jsonl").

%% `audit`: throughput by sink, producer count and payload size.
-spec audit(boolean()) -> ok.
audit(Quick) ->
    hird_bench:title(
        "audit",
        "N dispatches split over producers; written/s is N over the time to "
        "sync's return, offered/s N over the producers' time; the stdout "
        "sink writes to /dev/null",
        ["case", "written/s", "offered/s", "p50", "p99", "max", "queue",
         "memory"]),
    _ = [audit_case(Kind, Producers, Size, records(Size, Quick))
         || Kind <- [file, stdout], Producers <- [1, 16],
            Size <- [16, 4096, 65536]],
    ok.

%% Records per audit case, sized so a slow sink still finishes each in
%% seconds.
records(Size, Quick) ->
    Full = case Size of
        16 -> 100000;
        4096 -> 20000;
        65536 -> 2000
    end,
    case Quick of
        true -> Full div 50;
        false -> Full
    end.

%% One audit row.
audit_case(Kind, Producers, Size, Total) ->
    Sink = start_sink(Kind),
    Hist = hird_bench:hist(),
    Sampler = hird_bench:sampler(fun() -> hird_bench:probe(Sink) end, 10),
    {Start, Offered} = burst(Producers, Total, payload(Size), Hist),
    ok = hird_audit:sync(),
    Written = erlang:monotonic_time(),
    {Queue, Memory} = hird_bench:peaks(hird_bench:stop(Sampler)),
    ok = stop_sink(Sink, Kind, Total),
    hird_bench:row(
        [lists:flatten(io_lib:format("~s ~bp ~s",
                                     [Kind, Producers, size_label(Size)])),
         hird_bench:rate(Total / hird_bench:seconds(Written - Start)),
         hird_bench:rate(Total / hird_bench:seconds(Offered - Start))]
        ++ hird_bench:latency(Hist)
        ++ [hird_bench:int(Queue), hird_bench:mib(Memory)]).

%% `sync`: how long sync waits behind the backlog a burst leaves.
-spec sync(boolean()) -> ok.
sync(Quick) ->
    hird_bench:title(
        "sync",
        "a burst of N 16 B dispatches from one producer, then sync; queued "
        "is the sink's queue when sync is called (median), p50 to max "
        "the sync wait",
        ["burst", "written/s", "queued", "p50", "p99", "max", "runs",
         "memory"]),
    Cases = case Quick of
        true -> [{1000, 2}, {10000, 2}];
        false -> [{1000, 10}, {10000, 10}, {100000, 3}]
    end,
    _ = [sync_case(Burst, Runs) || {Burst, Runs} <- Cases],
    ok.

%% One sync row: `Runs` bursts of `Burst` records into one file sink.
sync_case(Burst, Runs) ->
    Sink = start_sink(file),
    Hist = hird_bench:hist(),
    Sampler = hird_bench:sampler(fun() -> hird_bench:probe(Sink) end, 10),
    Results = [sync_run(Sink, Burst, Hist) || _ <- lists:seq(1, Runs)],
    {_, Memory} = hird_bench:peaks(hird_bench:stop(Sampler)),
    ok = stop_sink(Sink, file, Burst * Runs),
    Elapsed = lists:sum([E || {E, _} <- Results]),
    Queued = lists:nth((Runs + 1) div 2, lists:sort([Q || {_, Q} <- Results])),
    hird_bench:row(
        [hird_bench:int(Burst),
         hird_bench:rate(Burst * Runs / hird_bench:seconds(Elapsed)),
         hird_bench:int(Queued)]
        ++ hird_bench:latency(Hist)
        ++ [hird_bench:int(Runs), hird_bench:mib(Memory)]).

%% One burst and sync: the time from the burst's start to sync's return,
%% and the queue sync found; the sync wait goes into `Hist`.
sync_run(Sink, Burst, Hist) ->
    {Start, _} = burst(1, Burst, payload(16), hird_bench:hist()),
    {message_queue_len, Queued} = process_info(Sink, message_queue_len),
    Called = erlang:monotonic_time(),
    ok = hird_audit:sync(),
    Returned = erlang:monotonic_time(),
    hird_bench:record(Hist, Returned - Called),
    {Returned - Start, Queued}.

%% `mailbox`: the sink's queue and memory over time under a producer
%% paced at 10k and at 200k calls per second.
-spec mailbox(boolean()) -> ok.
mailbox(Quick) ->
    {Ms, Interval} = case Quick of
        true -> {200, 50};
        false -> {2000, 250}
    end,
    hird_bench:title(
        "mailbox",
        lists:flatten(io_lib:format(
            "one producer paced at a target rate for ~b ms, 16 B payloads, "
            "file sink, then sync; drain is the wait after the producer "
            "stops", [Ms])),
        ["rate", "offered/s", "written/s", "p50", "p99", "max", "queue",
         "memory", "drain"]),
    Series = [mailbox_case(Rate, Ms, Interval) || Rate <- [10000, 200000]],
    _ = [print_series(Rate, Samples) || {Rate, Samples} <- Series],
    ok.

%% One mailbox row; its rate and samples for the series.
mailbox_case(Rate, Ms, Interval) ->
    Sink = start_sink(file),
    Hist = hird_bench:hist(),
    Sent = counters:new(1, []),
    Probe = fun() ->
        (hird_bench:probe(Sink))#{offered => counters:get(Sent, 1)}
    end,
    Sampler = hird_bench:sampler(Probe, Interval),
    Payload = payload(16),
    Handlers = handlers(),
    Call = fun(N) -> call(N, Payload, Handlers, Hist) end,
    {Start, Stopped} = hird_bench:parallel([fun() ->
        T0 = erlang:monotonic_time(),
        paced(Rate, T0, T0 + hird_bench:native(Ms), Call, Sent, 0)
    end]),
    ok = hird_audit:sync(),
    Written = erlang:monotonic_time(),
    Samples = hird_bench:stop(Sampler),
    {Queue, Memory} = hird_bench:peaks(Samples),
    Total = counters:get(Sent, 1),
    ok = stop_sink(Sink, file, Total),
    hird_bench:row(
        [hird_bench:rate(Rate) ++ "/s",
         hird_bench:rate(Total / hird_bench:seconds(Stopped - Start)),
         hird_bench:rate(Total / hird_bench:seconds(Written - Start))]
        ++ hird_bench:latency(Hist)
        ++ [hird_bench:int(Queue), hird_bench:mib(Memory),
            hird_bench:nanos(erlang:convert_time_unit(Written - Stopped,
                                                      native, nanosecond))]),
    {Rate, Samples}.

%% Runs `Call(N)` at `Rate` per second from `Start` until `Deadline`,
%% catching up after any stall and sleeping 1 ms when ahead; `N` calls
%% so far, also counted in `Sent` for the sampler.
paced(Rate, Start, Deadline, Call, Sent, N) ->
    Now = erlang:monotonic_time(),
    Due = hird_bench:seconds(Now - Start) * Rate,
    if
        Now >= Deadline ->
            ok;
        Due >= N + 1 ->
            Call(N),
            counters:add(Sent, 1, 1),
            paced(Rate, Start, Deadline, Call, Sent, N + 1);
        true ->
            receive after 1 -> ok end,
            paced(Rate, Start, Deadline, Call, Sent, N)
    end.

%% Prints one rate's samples: dispatches so far, the sink's queue and
%% memory, and node memory growth.
print_series(Rate, [{_, #{node := Base}} | _] = Samples) ->
    io:format(standard_error, "~n  ~s/s over time~n", [hird_bench:rate(Rate)]),
    hird_bench:row(["  ms", "offered", "queue", "sink", "memory"]),
    _ = [hird_bench:row(["  " ++ hird_bench:int(T), hird_bench:int(Sent),
                         hird_bench:int(Q), hird_bench:mib(P),
                         hird_bench:mib(N - Base)])
         || {T, #{offered := Sent, queue := Q, process := P, node := N}}
                <- Samples],
    ok.

%% Producers -----------------------------------------------------------------

%% Dispatches `Total` calls split over `Producers` processes released
%% together, sampled calls timed into `Hist`; the monotonic times at
%% release and once the last producer is done.
burst(Producers, Total, Payload, Hist) ->
    Handlers = handlers(),
    hird_bench:parallel(
        [fun() -> produce(Share, Payload, Handlers, Hist) end
         || Share <- shares(Total, Producers)]).

%% `Total` split into `N` near-equal shares.
shares(Total, N) ->
    [Total div N + min(1, max(0, Total rem N - I)) || I <- lists:seq(0, N - 1)].

%% `N` dispatches.
produce(0, _Payload, _Handlers, _Hist) ->
    ok;
produce(N, Payload, Handlers, Hist) ->
    call(N, Payload, Handlers, Hist),
    produce(N - 1, Payload, Handlers, Hist).

%% The `N`th dispatch, of a fresh copy of `Payload`.
call(N, Payload, Handlers, Hist) ->
    Args = binary:copy(Payload),
    0 = hird_bench:timed(N, Hist, fun() ->
        hird_tool_dispatch:call(echo, [], <<"Bench.producer">>, Handlers, Args)
    end).

%% The threaded handler map: `echo` answers 0.
handlers() ->
    #{{tool, echo} => fun(_Args, _Handlers) -> 0 end}.

%% `Echo : String → Int`.
table() ->
    #{tools => #{echo => #{name => <<"Echo">>, params => 0, args => string,
                           result => int, error => dynamic}},
      types => #{}}.

%% An ASCII payload of `Size` bytes, nothing to escape.
payload(Size) ->
    binary:copy(<<"a">>, Size).

%% A payload size for a case label.
size_label(Size) when Size >= 1024 -> integer_to_list(Size div 1024) ++ "KiB";
size_label(Size) -> integer_to_list(Size) ++ "B".

%% Sinks ---------------------------------------------------------------------

%% Starts a sink the way the boot module does, the file sink on an empty
%% log.
start_sink(Kind) ->
    Sink = case Kind of
        file ->
            _ = file:delete(?LOG),
            {file, ?LOG};
        stdout ->
            stdout
    end,
    {ok, Pid} = hird_audit:start_link([{sink, Sink}]),
    ok = hird_audit:register_tools(table()),
    Pid.

%% Stops a sink; the file sink must have written all `Total` records,
%% so no figure counts records it lost.
stop_sink(Pid, Kind, Total) ->
    ok = gen_server:stop(Pid),
    case Kind of
        file ->
            {ok, Bytes} = file:read_file(?LOG),
            ok = file:delete(?LOG),
            case length(binary:matches(Bytes, <<"\n">>)) of
                Total -> ok;
                Lines -> erlang:error({lost_records, Total - Lines})
            end;
        stdout ->
            ok
    end.

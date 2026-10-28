%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Audit sink behaviour: JSON-lines file output in arrival order, table
%% registration, append-across-restarts, an unbounded sync, encoding in the
%% caller, batched writes, and a bounded backlog.
-module(hird_audit_tests).

-include_lib("eunit/include/eunit.hrl").

table() ->
    #{tools => #{ping => #{name => <<"Ping">>, params => 0, args => int,
                           result => int, error => dynamic},
                 shout => #{name => <<"Shout">>, params => 0, args => string,
                            result => int, error => dynamic}},
      types => #{}}.

record_at(N) ->
    #{tool => ping, type_args => [], args => N, result => {ok, N},
      timestamp => 1000 * N, caller => <<"M.f">>}.

fresh(Name) ->
    Path = filename:join("_build", Name),
    _ = file:delete(Path),
    Path.

writes_ordered_json_lines_test() ->
    Path = fresh("audit_ordered.jsonl"),
    {ok, Sink} = hird_audit:start_link([{sink, {file, Path}}, {tools, table()}]),
    lists:foreach(fun(N) -> ok = hird_audit:log(record_at(N)) end,
                  lists:seq(1, 5)),
    ok = hird_audit:sync(),
    gen_server:stop(Sink),
    {ok, Bytes} = file:read_file(Path),
    Lines = [L || L <- binary:split(Bytes, <<"\n">>, [global]), L =/= <<>>],
    ?assertEqual(5, length(Lines)),
    ?assertEqual(
        [integer_to_binary(N) || N <- lists:seq(1, 5)],
        [begin
             [_, Rest] = binary:split(L, <<"\"args\":">>),
             [Args, _] = binary:split(Rest, <<",">>),
             Args
         end || L <- Lines]).

register_tools_extends_the_table_test() ->
    Path = fresh("audit_registered.jsonl"),
    {ok, Sink} = hird_audit:start_link([{sink, {file, Path}}]),
    ok = hird_audit:register_tools(table()),
    ok = hird_audit:log(record_at(7)),
    ok = hird_audit:sync(),
    gen_server:stop(Sink),
    {ok, Bytes} = file:read_file(Path),
    ?assertMatch({match, _}, re:run(Bytes, <<"\"tool\":\"Ping\"">>)).

file_sink_appends_across_restarts_test() ->
    Path = fresh("audit_appended.jsonl"),
    lists:foreach(
        fun(N) ->
            {ok, Sink} =
                hird_audit:start_link([{sink, {file, Path}}, {tools, table()}]),
            ok = hird_audit:log(record_at(N)),
            ok = hird_audit:sync(),
            gen_server:stop(Sink)
        end,
        [1, 2]),
    {ok, Bytes} = file:read_file(Path),
    Lines = [L || L <- binary:split(Bytes, <<"\n">>, [global]), L =/= <<>>],
    ?assertEqual(2, length(Lines)).

log_without_a_running_sink_is_a_noop_test() ->
    ?assertEqual(undefined, whereis(hird_audit)),
    ?assertEqual(ok, hird_audit:log(record_at(1))).

%% A backlog that outlasts gen_server's default 5 s call timeout: the sink
%% is suspended past it, and sync still returns once the record lands.
sync_outwaits_the_default_call_timeout_test_() ->
    {timeout, 30, fun() ->
        Path = fresh("audit_slow.jsonl"),
        {ok, Sink} =
            hird_audit:start_link([{sink, {file, Path}}, {tools, table()}]),
        ok = sys:suspend(Sink),
        ok = hird_audit:log(record_at(1)),
        Self = self(),
        spawn(fun() -> Self ! {synced, catch hird_audit:sync()} end),
        timer:sleep(5500),
        ok = sys:resume(Sink),
        receive {synced, Result} -> ?assertEqual(ok, Result) end,
        gen_server:stop(Sink),
        {ok, Bytes} = file:read_file(Path),
        ?assertMatch({match, _}, re:run(Bytes, <<"\"args\":1">>))
    end}.

%% The lines in `Path`.
lines(Path) ->
    {ok, Bytes} = file:read_file(Path),
    [L || L <- binary:split(Bytes, <<"\n">>, [global]), L =/= <<>>].

%% A burst far past the backlog bound: every record is on disk once sync
%% returns, read before the sink stops, in call order.
burst_is_written_in_full_by_sync_test_() ->
    {timeout, 120, fun() ->
        Path = fresh("audit_burst.jsonl"),
        {ok, Sink} =
            hird_audit:start_link([{sink, {file, Path}}, {tools, table()}]),
        Total = 200000,
        lists:foreach(fun(N) -> ok = hird_audit:log(record_at(N)) end,
                      lists:seq(1, Total)),
        ok = hird_audit:sync(),
        Lines = lines(Path),
        gen_server:stop(Sink),
        ?assertEqual(Total, length(Lines)),
        ?assertEqual(lists:seq(1, Total),
                     [begin
                          [_, Rest] = binary:split(L, <<"\"args\":">>),
                          [Args, _] = binary:split(Rest, <<",">>),
                          binary_to_integer(Args)
                      end || L <- Lines])
    end}.

%% A logged line reaches the file on its own, without a sync.
idle_sink_writes_without_a_sync_test() ->
    Path = fresh("audit_idle.jsonl"),
    {ok, Sink} = hird_audit:start_link([{sink, {file, Path}}, {tools, table()}]),
    ok = hird_audit:log(record_at(1)),
    Written = wait_until(fun() -> length(lines(Path)) =:= 1 end, 100),
    gen_server:stop(Sink),
    ?assert(Written).

%% While the sink is suspended, a producer of 1 MiB lines casts until the
%% backlog bound, then waits in a call; resuming the sink releases it and
%% every line is written.
backlog_bound_blocks_the_producer_test_() ->
    {timeout, 60, fun() ->
        Path = fresh("audit_backlog.jsonl"),
        {ok, Sink} =
            hird_audit:start_link([{sink, {file, Path}}, {tools, table()}]),
        ok = sys:suspend(Sink),
        Payload = binary:copy(<<"a">>, 1 bsl 20),
        Total = 16,
        Self = self(),
        Producer = spawn_link(fun() ->
            lists:foreach(
                fun(N) ->
                    ok = hird_audit:log(#{tool => shout, type_args => [],
                                          args => Payload, result => {ok, N},
                                          timestamp => N, caller => <<"M.f">>})
                end,
                lists:seq(1, Total)),
            Self ! {produced, self()}
        end),
        %% The producer encodes the 8 MiB it casts before it blocks, which
        %% takes seconds on a slow host: wait up to 30 s of the 60.
        ?assert(wait_until(fun() -> lists:any(fun is_log_call/1,
                                              queued(Sink)) end, 3000)),
        Casts = length(queued(Sink)) - 1,
        ?assert(Casts < Total - 1),
        receive {produced, Producer} -> ?assert(false) after 0 -> ok end,
        ok = sys:resume(Sink),
        receive {produced, Producer} -> ok end,
        ok = hird_audit:sync(),
        Lines = lines(Path),
        gen_server:stop(Sink),
        ?assertEqual(Total, length(Lines))
    end}.

%% A record the tables cannot encode fails in the caller; the sink keeps
%% running and writes the next record.
unencodable_record_fails_in_the_caller_test() ->
    Path = fresh("audit_unencodable.jsonl"),
    {ok, Sink} = hird_audit:start_link([{sink, {file, Path}}, {tools, table()}]),
    ?assertError({unknown_tool, ghost},
                 hird_audit:log((record_at(1))#{tool := ghost})),
    ok = hird_audit:log(record_at(2)),
    ok = hird_audit:sync(),
    Lines = lines(Path),
    gen_server:stop(Sink),
    ?assertEqual(1, length(Lines)).

log_after_the_sink_stops_is_a_noop_test() ->
    Path = fresh("audit_stopped.jsonl"),
    {ok, Sink} = hird_audit:start_link([{sink, {file, Path}}, {tools, table()}]),
    gen_server:stop(Sink),
    ?assertEqual(ok, hird_audit:log(record_at(1))),
    ?assertEqual([], lines(Path)).

%% The messages queued at `Pid`.
queued(Pid) ->
    {messages, Messages} = process_info(Pid, messages),
    Messages.

is_log_call({'$gen_call', _From, {log, _Line}}) -> true;
is_log_call(_) -> false.

%% Polls `Done` every 10 ms, at most `Tries` times.
wait_until(Done, Tries) ->
    case Done() of
        true -> true;
        false when Tries =:= 0 -> false;
        false -> timer:sleep(10), wait_until(Done, Tries - 1)
    end.

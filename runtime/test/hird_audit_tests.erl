%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Audit sink behaviour: JSON-lines file output in arrival order, table
%% registration, append-across-restarts, and an unbounded sync.
-module(hird_audit_tests).

-include_lib("eunit/include/eunit.hrl").

table() ->
    #{tools => #{ping => #{name => <<"Ping">>, params => 0, args => int,
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

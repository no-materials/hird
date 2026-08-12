%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Dispatcher routing: threaded map first, registry fallback second,
%% `{unhandled_tool, _}` crash third — and unconditional audit capture.
-module(hird_tool_dispatch_tests).

-include_lib("eunit/include/eunit.hrl").

threaded_map_entry_wins_test() ->
    Handlers = #{{tool, echo} => fun(Args, _Handlers) -> {echoed, Args} end},
    ?assertEqual({echoed, 7},
                 hird_tool_dispatch:call(echo, <<"M.f">>, Handlers, 7)).

handler_receives_the_threaded_map_test() ->
    Handlers = #{{tool, spy} => fun(_Args, Map) -> maps:size(Map) end},
    ?assertEqual(1, hird_tool_dispatch:call(spy, <<"M.f">>, Handlers, ok)).

registry_fallback_on_map_miss_test() ->
    hird_handlers:with_handlers(
        [{{tool, fallback}, fun(Args, _Handlers) -> Args + 1 end}],
        fun() ->
            ?assertEqual(42,
                         hird_tool_dispatch:call(fallback, <<"M.f">>, #{}, 41))
        end).

unhandled_tool_crashes_test() ->
    ?assertError({unhandled_tool, ghost},
                 hird_tool_dispatch:call(ghost, <<"M.f">>, #{}, ok)).

%% A domain failure — `throw({hird_exn, Error})` — is rethrown to the
%% caller unchanged; audit capture is observational.
exn_throw_propagates_to_the_caller_test() ->
    Handlers = #{{tool, failing} =>
                     fun(_Args, _Handlers) -> throw({hird_exn, bad_input}) end},
    ?assertThrow({hird_exn, bad_input},
                 hird_tool_dispatch:call(failing, <<"M.f">>, Handlers, ok)).

dispatch_works_without_an_audit_sink_test() ->
    ?assertEqual(undefined, whereis(hird_audit)),
    Handlers = #{{tool, quiet} => fun(_Args, _Handlers) -> ok end},
    ?assertEqual(ok, hird_tool_dispatch:call(quiet, <<"M.f">>, Handlers, ok)).

%% A mocked call produces a full invocation record at the sink: right tool
%% wire name, args, result, and the codegen-supplied caller.
audit_captures_mocked_invocations_test() ->
    Path = filename:join("_build", "dispatch_audit.jsonl"),
    _ = file:delete(Path),
    Table = #{tools => #{probe => #{name => <<"Probe">>,
                                    args => {record, [{n, int}]},
                                    result => int,
                                    error => dynamic}},
              types => #{}},
    {ok, Sink} = hird_audit:start_link([{sink, {file, Path}}, {tools, Table}]),
    Handlers = #{{tool, probe} => fun(#{n := N}, _Handlers) -> N * 2 end},
    ?assertEqual(6, hird_tool_dispatch:call(probe, <<"M.f">>, Handlers, #{n => 3})),
    ok = hird_audit:sync(),
    gen_server:stop(Sink),
    {ok, Bytes} = file:read_file(Path),
    [Line, <<>>] = binary:split(Bytes, <<"\n">>),
    ?assertMatch({match, _},
                 re:run(Line,
                        <<"^\\{\"schema_version\":1,\"tool\":\"Probe\","
                          "\"args\":\\{\"n\":3\\},\"result\":\\{\"ok\":6\\},"
                          "\"timestamp\":\"[0-9T:.Z-]+\","
                          "\"caller\":\"M\\.f\"\\}$">>)).

%% A failing handler produces an err-tagged line byte-identical to the
%% encoder's output for the same record — the same tool, args, error
%% value, and caller as the http_get_err.json golden, which pins the
%% encoder against the oracle bytes. Only the injected timestamp (read
%% back from the line) and the observer-populated meta differ.
audit_captures_err_results_test() ->
    Path = filename:join("_build", "dispatch_audit_err.jsonl"),
    _ = file:delete(Path),
    Table = #{tools => #{http_get => #{name => <<"HttpGet">>,
                                       args => {record, [{url, string}]},
                                       result => {record, [{status, int}]},
                                       error => {adt, http_error, []}}},
              types => #{http_error =>
                             [{http_error, <<"HttpError">>, [int, string]}]}},
    {ok, Sink} = hird_audit:start_link([{sink, {file, Path}}, {tools, Table}]),
    Error = {http_error, 503, <<"service unavailable">>},
    Args = #{url => <<"https://ci.example/status">>},
    Handlers = #{{tool, http_get} =>
                     fun(_Args, _Handlers) -> throw({hird_exn, Error}) end},
    ?assertThrow({hird_exn, Error},
                 hird_tool_dispatch:call(http_get, <<"Planner.check_ci">>,
                                         Handlers, Args)),
    ok = hird_audit:sync(),
    gen_server:stop(Sink),
    {ok, Bytes} = file:read_file(Path),
    [Line, <<>>] = binary:split(Bytes, <<"\n">>),
    [_, Tail] = binary:split(Line, <<"\"timestamp\":\"">>),
    [Rfc3339, _] = binary:split(Tail, <<"\"">>),
    Ts = calendar:rfc3339_to_system_time(binary_to_list(Rfc3339),
                                         [{unit, millisecond}]),
    Expected = hird_types:encode_invocation(
        #{tool => http_get, args => Args, result => {err, Error},
          timestamp => Ts, caller => <<"Planner.check_ci">>},
        Table),
    ?assertEqual(Expected, Line).

%% A crash in a handler is not a domain error: it propagates untouched
%% and leaves no audit record.
crashes_propagate_unrecorded_test() ->
    Path = filename:join("_build", "dispatch_audit_crash.jsonl"),
    _ = file:delete(Path),
    {ok, Sink} = hird_audit:start_link([{sink, {file, Path}}]),
    Handlers = #{{tool, exploder} =>
                     fun(_Args, _Handlers) -> erlang:error(boom) end},
    ?assertError(boom,
                 hird_tool_dispatch:call(exploder, <<"M.f">>, Handlers, ok)),
    ok = hird_audit:sync(),
    gen_server:stop(Sink),
    ?assertEqual({ok, <<>>}, file:read_file(Path)).

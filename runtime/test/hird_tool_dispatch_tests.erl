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

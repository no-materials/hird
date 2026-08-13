%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Dispatcher routing: threaded map first, registry fallback second,
%% `{unhandled_tool, _}` crash third — and unconditional audit capture.
%% Under a running replay cursor, none of that: the cursor alone answers.
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

%% Replay ------------------------------------------------------------------

%% The `Probe : { n: Int } → Int` signature table.
probe_table() ->
    #{tools => #{probe => #{name => <<"Probe">>,
                            args => {record, [{n, int}]},
                            result => int,
                            error => {adt, probe_error, []}}},
      types => #{probe_error => [{probe_error, <<"ProbeError">>, [string]}]}}.

%% Runs `Fun` with a replay cursor over `probe` records, stopping it
%% afterwards.
with_replay(Name, Records, Fun) ->
    Path = filename:join("_build", Name),
    Lines = [hird_types:encode_invocation(
                 #{tool => probe, args => #{n => N}, result => Result,
                   timestamp => 0, caller => <<"M.f">>},
                 probe_table())
             || {N, Result} <- Records],
    ok = file:write_file(Path, [[L, $\n] || L <- Lines]),
    {ok, Pid} = hird_replay:start_link(Path, [probe_table()]),
    try
        Fun()
    after
        gen_server:stop(Pid)
    end.

%% Under replay the cursor is the only authority: a threaded handler for
%% the same tool is never invoked, so a `handle` block in the program
%% cannot shadow the log.
replay_ignores_threaded_handlers_test() ->
    Handlers = #{{tool, probe} => fun(_Args, _Handlers) -> shadowed end},
    with_replay("dispatch_replay_shadow.jsonl", [{3, {ok, 99}}], fun() ->
        ?assertEqual(99,
                     hird_tool_dispatch:call(probe, <<"M.f">>, Handlers,
                                             #{n => 3}))
    end).

%% Same for the registry: an installed default loses to the cursor.
replay_ignores_registry_handlers_test() ->
    hird_handlers:with_handlers(
        [{{tool, probe}, fun(_Args, _Handlers) -> shadowed end}],
        fun() ->
            with_replay("dispatch_replay_registry.jsonl", [{3, {ok, 99}}],
                        fun() ->
                            ?assertEqual(99,
                                         hird_tool_dispatch:call(
                                             probe, <<"M.f">>, #{}, #{n => 3}))
                        end)
        end).

%% An err-tagged record replays its failure: the same `{hird_exn, _}`
%% throw the recorded handler raised.
replay_rethrows_logged_failures_test() ->
    Error = {probe_error, <<"down">>},
    with_replay("dispatch_replay_err.jsonl", [{3, {err, Error}}], fun() ->
        ?assertThrow({hird_exn, Error},
                     hird_tool_dispatch:call(probe, <<"M.f">>, #{}, #{n => 3}))
    end).

%% A mismatching dispatch crashes with the structured divergence.
replay_divergence_crashes_test() ->
    with_replay("dispatch_replay_diverge.jsonl", [{3, {ok, 99}}], fun() ->
        ?assertError({replay_divergence, #{kind := args_mismatch,
                                           position := 0}},
                     hird_tool_dispatch:call(probe, <<"M.f">>, #{}, #{n => 4}))
    end).

%% Replayed dispatches audit exactly like live ones: same tool, args,
%% and result on the stream, ok and err alike.
replay_still_audits_test() ->
    Path = filename:join("_build", "dispatch_replay_audit.jsonl"),
    _ = file:delete(Path),
    {ok, Sink} = hird_audit:start_link([{sink, {file, Path}},
                                        {tools, probe_table()}]),
    Error = {probe_error, <<"down">>},
    with_replay("dispatch_replay_audited_log.jsonl",
                [{3, {ok, 99}}, {4, {err, Error}}],
                fun() ->
                    ?assertEqual(99,
                                 hird_tool_dispatch:call(probe, <<"M.f">>, #{},
                                                         #{n => 3})),
                    ?assertThrow({hird_exn, Error},
                                 hird_tool_dispatch:call(probe, <<"M.f">>, #{},
                                                         #{n => 4}))
                end),
    ok = hird_audit:sync(),
    gen_server:stop(Sink),
    {ok, Bytes} = file:read_file(Path),
    [First, Second, <<>>] = binary:split(Bytes, <<"\n">>, [global]),
    ?assertMatch({match, _},
                 re:run(First, <<"\"args\":\\{\"n\":3\\},"
                                 "\"result\":\\{\"ok\":99\\}">>)),
    ?assertMatch({match, _},
                 re:run(Second,
                        <<"\"args\":\\{\"n\":4\\},\"result\":\\{\"err\":"
                          "\\{\"ctor\":\"ProbeError\","
                          "\"args\":\\[\"down\"\\]\\}\\}">>)).

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

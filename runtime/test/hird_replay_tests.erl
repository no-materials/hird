%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Cursor semantics: strict-sequential matching over a decoded log file,
%% divergences that never advance, the whole-log finish check,
%% structured load failures, and a generic tool's records decoded at the
%% type arguments of the call offered against them.
-module(hird_replay_tests).

-include_lib("eunit/include/eunit.hrl").

%% A `Ping : { n: Int } → Int ! Exn<PingError>` signature table.
table() ->
    #{tools => #{ping => #{name => <<"Ping">>,
                           params => 0,
                           args => {record, [{n, int}]},
                           result => int,
                           error => {adt, ping_error, []}}},
      types => #{ping_error => [{ping_error, <<"PingError">>, [string]}]}}.

%% One encoded log line for a `ping` invocation.
line(N, Result) ->
    hird_types:encode_invocation(
        #{tool => ping, type_args => [], args => #{n => N}, result => Result,
          timestamp => 0, caller => <<"M.f">>},
        table()).

%% Writes `Lines` as a log file under _build, returning its path.
log_file(Name, Lines) ->
    Path = filename:join("_build", Name),
    ok = file:write_file(Path, [[L, $\n] || L <- Lines]),
    Path.

%% Runs `Fun` against a cursor over `Lines`, stopping it afterwards.
with_cursor(Name, Lines, Fun) ->
    {ok, Pid} = hird_replay:start_link(log_file(Name, Lines), [table()]),
    try
        Fun()
    after
        gen_server:stop(Pid)
    end.

active_tracks_the_cursor_process_test() ->
    ?assertNot(hird_replay:active()),
    with_cursor("replay_active.jsonl", [],
                fun() -> ?assert(hird_replay:active()) end),
    ?assertNot(hird_replay:active()).

matches_yield_logged_results_in_order_test() ->
    Lines = [line(1, {ok, 10}),
             line(2, {err, {ping_error, <<"down">>}})],
    with_cursor("replay_order.jsonl", Lines, fun() ->
        ?assertEqual({ok, 10}, hird_replay:offer(ping, [], #{n => 1})),
        ?assertEqual({err, {ping_error, <<"down">>}},
                     hird_replay:offer(ping, [], #{n => 2})),
        ?assertEqual(ok, hird_replay:finish())
    end).

a_divergence_does_not_advance_the_cursor_test() ->
    with_cursor("replay_no_advance.jsonl", [line(1, {ok, 10})], fun() ->
        ?assertMatch({diverged, #{kind := args_mismatch, position := 0,
                                  expected := #{tool := ping, args := #{n := 1}},
                                  offered := #{tool := ping, args := #{n := 9}}}},
                     hird_replay:offer(ping, [], #{n => 9})),
        ?assertEqual({ok, 10}, hird_replay:offer(ping, [], #{n => 1}))
    end).

a_wrong_tool_is_a_tool_mismatch_test() ->
    with_cursor("replay_tool_mismatch.jsonl", [line(1, {ok, 10})], fun() ->
        ?assertMatch({diverged, #{kind := tool_mismatch, position := 0,
                                  log_size := 1}},
                     hird_replay:offer(pong, [], #{n => 1}))
    end).

an_exhausted_log_is_a_divergence_test() ->
    with_cursor("replay_exhausted.jsonl", [line(1, {ok, 10})], fun() ->
        ?assertEqual({ok, 10}, hird_replay:offer(ping, [], #{n => 1})),
        ?assertMatch({diverged, #{kind := log_exhausted, position := 1,
                                  log_size := 1}},
                     hird_replay:offer(ping, [], #{n => 2}))
    end).

finish_reports_unconsumed_records_test() ->
    Lines = [line(1, {ok, 10}), line(2, {ok, 20})],
    with_cursor("replay_incomplete.jsonl", Lines, fun() ->
        ?assertEqual({ok, 10}, hird_replay:offer(ping, [], #{n => 1})),
        ?assertEqual({error, {replay_incomplete,
                              #{consumed => 1, log_size => 2}}},
                     hird_replay:finish())
    end).

%% Load-failure tests trap exits: a failed `init` exits the linked
%% starter as well as returning `{error, _}`, and eunit runs each test in
%% its own process, so the flag does not leak.

a_tampered_line_fails_the_load_with_its_line_number_test() ->
    process_flag(trap_exit, true),
    Path = log_file("replay_tampered.jsonl",
                    [line(1, {ok, 10}), <<"not json">>]),
    ?assertMatch({error, {replay_load_error,
                          #{line := 2, reason := {decode_error, _}}}},
                 hird_replay:start_link(Path, [table()])),
    ?assertNot(hird_replay:active()).

a_missing_file_fails_the_load_test() ->
    process_flag(trap_exit, true),
    ?assertMatch({error, {replay_load_error, #{reason := enoent}}},
                 hird_replay:start_link("_build/replay_missing.jsonl",
                                        [table()])).

%% Generic tools -----------------------------------------------------------

%% `Ping` beside an `Echo<t> : { v: t } → t`.
generic_table() ->
    #{tools => maps:put(echo, #{name => <<"Echo">>, params => 1,
                                args => {record, [{v, {param, 0}}]},
                                result => {param, 0},
                                error => dynamic},
                        maps:get(tools, table())),
      types => maps:get(types, table())}.

%% One encoded log line for an `echo` of `V` at `TypeArgs`.
echo_line(TypeArgs, V) ->
    hird_types:encode_invocation(
        #{tool => echo, type_args => TypeArgs, args => #{v => V},
          result => {ok, V}, timestamp => 0, caller => <<"M.f">>},
        generic_table()).

%% Runs `Fun` against a cursor over `Lines` under the generic table.
with_generic_cursor(Name, Lines, Fun) ->
    {ok, Pid} = hird_replay:start_link(log_file(Name, Lines),
                                       [generic_table()]),
    try
        Fun()
    after
        gen_server:stop(Pid)
    end.

%% A generic tool's records decode at the type arguments each offer
%% supplies, interleaved with ordinary records decoded at load.
generic_records_decode_at_the_offered_type_arguments_test() ->
    Lines = [echo_line([int], 1), line(2, {ok, 20}),
             echo_line([{list, string}], [<<"a">>])],
    with_generic_cursor("replay_generic.jsonl", Lines, fun() ->
        ?assertEqual({ok, 1}, hird_replay:offer(echo, [int], #{v => 1})),
        ?assertEqual({ok, 20}, hird_replay:offer(ping, [], #{n => 2})),
        ?assertEqual({ok, [<<"a">>]},
                     hird_replay:offer(echo, [{list, string}],
                                       #{v => [<<"a">>]})),
        ?assertEqual(ok, hird_replay:finish())
    end).

%% Recorded at `Int`, offered at `String`: the record does not decode
%% under the offer, so the call differs — args_mismatch, no logged args.
another_instantiation_is_an_args_mismatch_test() ->
    with_generic_cursor("replay_generic_type.jsonl", [echo_line([int], 1)],
                        fun() ->
        ?assertMatch({diverged, #{kind := args_mismatch, position := 0,
                                  expected := #{tool := echo},
                                  offered := #{tool := echo}}},
                     hird_replay:offer(echo, [string], #{v => <<"1">>})),
        {diverged, #{expected := Expected}} =
            hird_replay:offer(echo, [string], #{v => <<"1">>}),
        ?assertNot(maps:is_key(args, Expected)),
        ?assertMatch({diverged, #{kind := args_mismatch,
                                  expected := #{tool := echo,
                                                args := #{v := 1}}}},
                     hird_replay:offer(echo, [int], #{v => 2})),
        ?assertEqual({ok, 1}, hird_replay:offer(echo, [int], #{v => 1}))
    end).

%% Another tool offered over a generic record: a tool mismatch, the
%% record's args unknown without its call's type arguments.
another_tool_over_a_generic_record_is_a_tool_mismatch_test() ->
    with_generic_cursor("replay_generic_tool.jsonl", [echo_line([int], 1)],
                        fun() ->
        ?assertMatch({diverged, #{kind := tool_mismatch, position := 0,
                                  expected := #{tool := echo},
                                  offered := #{tool := ping}}},
                     hird_replay:offer(ping, [], #{n => 1}))
    end).

%% A generic record's envelope still fails the load, with its line number.
a_generic_record_s_envelope_is_validated_at_load_test() ->
    process_flag(trap_exit, true),
    Bad = binary:replace(echo_line([int], 1), <<"\"schema_version\":1">>,
                         <<"\"schema_version\":2">>),
    Path = log_file("replay_generic_bad.jsonl", [line(1, {ok, 10}), Bad]),
    ?assertMatch({error, {replay_load_error,
                          #{line := 2,
                            reason := {decode_error,
                                       {unsupported_schema_version, 2}}}}},
                 hird_replay:start_link(Path, [generic_table()])).

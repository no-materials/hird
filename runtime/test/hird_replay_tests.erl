%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Cursor semantics: strict-sequential matching over a decoded log file,
%% divergences that never advance, the whole-log finish check, and
%% structured load failures.
-module(hird_replay_tests).

-include_lib("eunit/include/eunit.hrl").

%% A `Ping : { n: Int } → Int ! Exn<PingError>` signature table.
table() ->
    #{tools => #{ping => #{name => <<"Ping">>,
                           args => {record, [{n, int}]},
                           result => int,
                           error => {adt, ping_error, []}}},
      types => #{ping_error => [{ping_error, <<"PingError">>, [string]}]}}.

%% One encoded log line for a `ping` invocation.
line(N, Result) ->
    hird_types:encode_invocation(
        #{tool => ping, args => #{n => N}, result => Result,
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
        ?assertEqual({ok, 10}, hird_replay:offer(ping, #{n => 1})),
        ?assertEqual({err, {ping_error, <<"down">>}},
                     hird_replay:offer(ping, #{n => 2})),
        ?assertEqual(ok, hird_replay:finish())
    end).

a_divergence_does_not_advance_the_cursor_test() ->
    with_cursor("replay_no_advance.jsonl", [line(1, {ok, 10})], fun() ->
        ?assertMatch({diverged, #{kind := args_mismatch, position := 0,
                                  expected := #{tool := ping, args := #{n := 1}},
                                  offered := #{tool := ping, args := #{n := 9}}}},
                     hird_replay:offer(ping, #{n => 9})),
        ?assertEqual({ok, 10}, hird_replay:offer(ping, #{n => 1}))
    end).

a_wrong_tool_is_a_tool_mismatch_test() ->
    with_cursor("replay_tool_mismatch.jsonl", [line(1, {ok, 10})], fun() ->
        ?assertMatch({diverged, #{kind := tool_mismatch, position := 0,
                                  log_size := 1}},
                     hird_replay:offer(pong, #{n => 1}))
    end).

an_exhausted_log_is_a_divergence_test() ->
    with_cursor("replay_exhausted.jsonl", [line(1, {ok, 10})], fun() ->
        ?assertEqual({ok, 10}, hird_replay:offer(ping, #{n => 1})),
        ?assertMatch({diverged, #{kind := log_exhausted, position := 1,
                                  log_size := 1}},
                     hird_replay:offer(ping, #{n => 2}))
    end).

finish_reports_unconsumed_records_test() ->
    Lines = [line(1, {ok, 10}), line(2, {ok, 20})],
    with_cursor("replay_incomplete.jsonl", Lines, fun() ->
        ?assertEqual({ok, 10}, hird_replay:offer(ping, #{n => 1})),
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

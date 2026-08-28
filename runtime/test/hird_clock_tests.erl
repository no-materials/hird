%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% schedule/4 against the calling process and against a live gen_server:
%% the delayed message arrives on the cast path, after the delay, and a
%% negative delay is refused.
-module(hird_clock_tests).
-behaviour(gen_server).

-include_lib("eunit/include/eunit.hrl").

-export([init/1, handle_call/3, handle_cast/2]).

%% @private A counter that records every cast it receives.
init(Test) ->
    {ok, {Test, 0}}.

%% @private
handle_call(count, _From, {Test, N}) ->
    {reply, N, {Test, N}}.

%% @private
handle_cast(tick, {Test, N}) ->
    Test ! {ticked, N + 1},
    {noreply, {Test, N + 1}}.

schedule_delivers_a_cast_after_the_delay_test() ->
    Clock = hird_clock:real(),
    Started = erlang:monotonic_time(millisecond),
    ok = hird_clock:schedule(Clock, self(), tick, 50),
    ?assertEqual(nothing_yet, receive {'$gen_cast', tick} -> early after 0 -> nothing_yet end),
    ?assertEqual(ok, receive {'$gen_cast', tick} -> ok after 1000 -> none end),
    ?assert(erlang:monotonic_time(millisecond) - Started >= 50).

scheduled_casts_reach_a_gen_server_test() ->
    {ok, Server} = gen_server:start_link(?MODULE, self(), []),
    Clock = hird_clock:real(),
    ok = hird_clock:schedule(Clock, Server, tick, 0),
    ok = hird_clock:schedule(Clock, Server, tick, 10),
    ?assertEqual(ok, receive {ticked, 1} -> ok after 1000 -> none end),
    ?assertEqual(ok, receive {ticked, 2} -> ok after 1000 -> none end),
    ?assertEqual(2, gen_server:call(Server, count)),
    unlink(Server),
    exit(Server, shutdown).

negative_delay_is_a_crash_test() ->
    ?assertError(function_clause, hird_clock:schedule(hird_clock:real(), self(), tick, -1)).

only_a_clock_schedules_test() ->
    ?assertError(function_clause, hird_clock:schedule(not_a_clock, self(), tick, 0)).

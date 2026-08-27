%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% await/0 against a live supervisor, with SIGTERM delivered the way the VM
%% delivers it: as an event on erl_signal_server. The test module doubles
%% as the supervisor callback module and provides its own trivial child.
-module(hird_stand_tests).
-behaviour(supervisor).

-include_lib("eunit/include/eunit.hrl").

-export([init/1, start_child_link/0]).

%% @private
init([]) ->
    Flags = #{strategy => one_for_one, intensity => 1, period => 5},
    Child = #{id => worker,
              start => {?MODULE, start_child_link, []},
              restart => permanent,
              type => worker},
    {ok, {Flags, [Child]}}.

%% @private
start_child_link() ->
    {ok, spawn_link(fun() -> receive stop -> ok end end)}.

%% A standing process: starts the tree, then awaits. Reports its pids and
%% the return of await/0 to the test.
stand(Test) ->
    {ok, Sup} = supervisor:start_link(?MODULE, []),
    {ok, Worker} = hird_sup_util:child_pid(Sup, worker),
    Test ! {standing, Sup, Worker},
    Result = hird_stand:await(),
    Test ! {returned, Result},
    receive stop -> ok end.

sigterm_stops_the_tree_then_returns_test() ->
    Test = self(),
    Standing = spawn_link(fun() -> stand(Test) end),
    {Sup, Worker} = receive {standing, S, W} -> {S, W} end,
    ok = wait_for_handler(50),
    %% The tree stands while no signal has arrived.
    ?assertEqual(timeout, receive {returned, _} -> early after 100 -> timeout end),
    ?assert(is_process_alive(Sup)),
    ok = gen_event:notify(erl_signal_server, sigterm),
    ?assertEqual({returned, ok}, receive M = {returned, _} -> M after 5000 -> none end),
    ?assertNot(is_process_alive(Sup)),
    ?assertNot(is_process_alive(Worker)),
    %% The standing process survives its supervisor's shutdown.
    ?assert(is_process_alive(Standing)),
    Standing ! stop,
    %% The default handler is back for the next test.
    ok = gen_event:swap_handler(erl_signal_server,
                                {hird_stand, []},
                                {erl_signal_handler, []}).

%% await/0 installs its handler asynchronously to this test; poll for it.
wait_for_handler(0) ->
    {error, no_handler};
wait_for_handler(N) ->
    case lists:member(hird_stand, gen_event:which_handlers(erl_signal_server)) of
        true -> ok;
        false -> timer:sleep(10), wait_for_handler(N - 1)
    end.

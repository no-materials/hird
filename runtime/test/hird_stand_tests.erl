%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% await/1 against a live supervisor, under each trigger: SIGTERM delivered
%% the way the VM delivers it (as an event on erl_signal_server; not on
%% Windows, which has no SIGTERM), and end of file on an io device. The
%% test module doubles as the supervisor callback module and provides its
%% own trivial child.
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

%% A standing process: starts the tree, then awaits the given triggers.
%% Reports its pids and the return of await/1 to the test.
stand(Test, Triggers) ->
    {ok, Sup} = supervisor:start_link(?MODULE, []),
    {ok, Worker} = hird_sup_util:child_pid(Sup, worker),
    Test ! {standing, Sup, Worker},
    Result = hird_stand:await(Triggers),
    Test ! {returned, Result},
    receive stop -> ok end.

%% The tree stands until `Fire` fires the trigger, then comes down in order
%% and await/1 returns to a standing process that outlives it.
stands_then_stops(Triggers, Fire) ->
    Test = self(),
    Standing = spawn_link(fun() -> stand(Test, Triggers) end),
    {Sup, Worker} = receive {standing, S, W} -> {S, W} end,
    ?assertEqual(timeout, receive {returned, _} -> early after 100 -> timeout end),
    ?assert(is_process_alive(Sup)),
    ok = Fire(),
    ?assertEqual({returned, ok}, receive M = {returned, _} -> M after 5000 -> none end),
    ?assertNot(is_process_alive(Sup)),
    ?assertNot(is_process_alive(Worker)),
    ?assert(is_process_alive(Standing)),
    Standing ! stop.

sigterm_stops_the_tree_then_returns_test_() ->
    case os:type() of
        {win32, _} -> [];
        _ -> [fun sigterm_stops_the_tree_then_returns/0]
    end.

sigterm_stops_the_tree_then_returns() ->
    stands_then_stops([sigterm], fun() ->
        ok = wait_for_handler(50),
        gen_event:notify(erl_signal_server, sigterm)
    end),
    %% The default handler is back for the next test.
    ok = gen_event:swap_handler(erl_signal_server,
                                {hird_stand, []},
                                {erl_signal_handler, []}).

%% End of file on the device stops the tree — and only end of file: input
%% before it is ignored, and the tree stands while the device stays open.
eof_stops_the_tree_then_returns_test() ->
    Device = open_device(),
    stands_then_stops([{eof, Device}], fun() -> Device ! close, ok end).

%% An io device standing in for the launcher's pipe: it answers the first
%% read with a line, holds every later read open until `close`, and then
%% answers each with end of file.
open_device() ->
    spawn(fun() -> device(first, []) end).

device(Phase, Waiting) ->
    receive
        {io_request, From, ReplyAs, _Request} when Phase =:= first ->
            From ! {io_reply, ReplyAs, <<"not a protocol
">>},
            device(open, Waiting);
        {io_request, From, ReplyAs, _Request} ->
            device(Phase, [{From, ReplyAs} | Waiting]);
        close ->
            lists:foreach(fun({From, ReplyAs}) -> From ! {io_reply, ReplyAs, eof} end, Waiting)
    end.

%% With no launcher argument, stdin is not a trigger: a node started by
%% hand never reads its standard input.
triggers_do_not_include_stdin_unasked_test() ->
    ?assertEqual(error, init:get_argument(hird_stop)),
    ?assertNot(lists:keymember(eof, 1, hird_stand:triggers())),
    ?assertEqual(os:type() =/= {win32, nt}, lists:member(sigterm, hird_stand:triggers())).

%% await/1 installs its handler asynchronously to this test; poll for it.
wait_for_handler(0) ->
    {error, no_handler};
wait_for_handler(N) ->
    case lists:member(hird_stand, gen_event:which_handlers(erl_signal_server)) of
        true -> ok;
        false -> timer:sleep(10), wait_for_handler(N - 1)
    end.

%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% child_pid/2 against a live supervisor. The test module doubles as the
%% supervisor callback module and provides its own trivial child.
-module(hird_sup_util_tests).
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

child_pid_finds_a_running_child_test() ->
    {ok, Sup} = supervisor:start_link(?MODULE, []),
    {ok, Pid} = hird_sup_util:child_pid(Sup, worker),
    ?assert(is_process_alive(Pid)),
    ?assertEqual(error, hird_sup_util:child_pid(Sup, missing)),
    unlink(Sup),
    exit(Sup, shutdown).

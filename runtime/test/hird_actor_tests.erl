%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Tests for hird_actor: Next-outcome dispatch.
-module(hird_actor_tests).

-include_lib("eunit/include/eunit.hrl").

continue_keeps_running_with_the_new_state_test() ->
    ?assertEqual({noreply, {st, 1}}, hird_actor:outcome({continue, {st, 1}}, {st, 0})).

stop_is_a_deliberate_normal_stop_with_the_incoming_state_test() ->
    ?assertEqual({stop, normal, {st, 0}}, hird_actor:outcome(stop, {st, 0})).

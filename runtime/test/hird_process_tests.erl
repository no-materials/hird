%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Process identity: naming a process, and the identities its spawns get.
%% Each test runs in a process of its own, since an identity lasts as long
%% as the process it names.
-module(hird_process_tests).

-include_lib("eunit/include/eunit.hrl").

a_process_is_unnamed_until_named_test_() ->
    {spawn, fun() ->
        ?assertEqual(undefined, hird_process:identity()),
        ok = hird_process:name(undefined),
        ?assertEqual(undefined, hird_process:identity()),
        ok = hird_process:name(<<"main">>),
        ?assertEqual(<<"main">>, hird_process:identity())
    end}.

%% Spawns are counted per actor, from 1, under the spawner's identity.
spawned_identities_count_per_actor_test_() ->
    {spawn, fun() ->
        ok = hird_process:name(<<"Sup/planner">>),
        ?assertEqual(<<"Sup/planner/Worker#1">>,
                     hird_process:spawned(<<"Worker">>)),
        ?assertEqual(<<"Sup/planner/Worker#2">>,
                     hird_process:spawned(<<"Worker">>)),
        ?assertEqual(<<"Sup/planner/Scout#1">>,
                     hird_process:spawned(<<"Scout">>))
    end}.

%% An unnamed process's spawns are unnamed too.
an_unnamed_spawner_names_nothing_test_() ->
    {spawn, fun() ->
        ?assertEqual(undefined, hird_process:spawned(<<"Worker">>))
    end}.

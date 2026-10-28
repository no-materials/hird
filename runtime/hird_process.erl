%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Process identity: the run-stable name of a Hirð process. The dispatcher
%% appends it to every invocation record's caller id, and replay matches
%% each process's tool calls against that process's records alone. A pid
%% differs from run to run; an identity does not, because it names the
%% process by how the program started it:
%%
%% - `main` — the process running `main`;
%% - `Sup/child` — the child `child` of supervisor `Sup` (a supervisor is a
%%   registered singleton and its child ids are distinct), kept across
%%   restarts;
%% - `Spawner/Actor#N` — the N-th `spawn` of `Actor` by the process named
%%   `Spawner`, counting from 1.
%%
%% Generated code names each process once, before its first tool call: the
%% boot module names `main`, and an actor's `init/1` names itself from the
%% identity its `start_link` was given. A process nothing named — one a
%% test or hand-written harness started — has no identity, and neither do
%% the processes it spawns.
-module(hird_process).

-export([name/1, identity/0, spawned/1]).

%% Names the calling process; `undefined` leaves it unnamed.
-spec name(binary() | undefined) -> ok.
name(undefined) ->
    ok;
name(Identity) ->
    put('$hird_process', Identity),
    ok.

%% The calling process's identity, `undefined` when it is unnamed.
-spec identity() -> binary() | undefined.
identity() ->
    get('$hird_process').

%% The identity of the next `Actor` the calling process spawns:
%% `Spawner/Actor#N`, N counting this process's spawns of `Actor`.
%% `undefined` when the calling process is unnamed.
-spec spawned(binary()) -> binary() | undefined.
spawned(Actor) ->
    case identity() of
        undefined ->
            undefined;
        Spawner ->
            Key = {'$hird_spawned', Actor},
            N = case get(Key) of
                undefined -> 1;
                Previous -> Previous + 1
            end,
            put(Key, N),
            <<Spawner/binary, $/, Actor/binary, $#, (integer_to_binary(N))/binary>>
    end.

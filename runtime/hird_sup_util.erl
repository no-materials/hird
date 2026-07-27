%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Supervisor utilities. Generated supervisor modules carry their child
%% specs inline, so this module holds only what generated code cannot:
%% reaching a supervised child's pid, since children are started
%% unregistered.
-module(hird_sup_util).

-export([child_pid/2]).

%% The pid of the running child registered under `Id` in `Sup`.
-spec child_pid(supervisor:sup_ref(), term()) -> {ok, pid()} | error.
child_pid(Sup, Id) ->
    case lists:keyfind(Id, 1, supervisor:which_children(Sup)) of
        {Id, Pid, _Type, _Modules} when is_pid(Pid) -> {ok, Pid};
        _ -> error
    end.

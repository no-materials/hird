%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Handler-outcome dispatch. A generated gen_server clause evaluates its
%% handler body to a Next outcome and hands it here together with the
%% incoming state. Keeping the case out of generated code keeps erlc's
%% cannot-match analysis quiet when a body visibly always continues.
-module(hird_actor).

-export([outcome/2]).

%% Maps a handler's Next outcome to the gen_server callback return:
%% `Continue(Next)` keeps the server running with the new state, `Stop`
%% stops it with reason `normal` and the incoming state. `normal` is what
%% makes the stop deliberate rather than a crash: a transient child stays
%% stopped, a permanent one is restarted.
-spec outcome({continue, State} | stop, State) ->
    {noreply, State} | {stop, normal, State}.
outcome({continue, Next}, _State) -> {noreply, Next};
outcome(stop, State) -> {stop, normal, State}.

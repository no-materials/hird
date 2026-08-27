%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Standing mode: keeps a program up after its entry function has done its
%% setup. await/0 blocks the calling process until the node receives
%% SIGTERM, then shuts down every supervisor the caller started and
%% returns, so the caller's own teardown (the boot module's audit sync)
%% runs after the trees are gone.
%%
%% Supervisors started from the calling process are its linked children
%% whose initial call is `supervisor` — exactly the trees `supervise`
%% starts, since it calls start_link/0 in the caller. They are stopped in
%% reverse start order by an exit signal from their parent, which is the
%% OTP shutdown protocol: each supervisor terminates its children within
%% their shutdown timeouts before exiting itself.
%%
%% SIGTERM is the only signal handled: the BEAM does not expose SIGINT to
%% Erlang code (its break handler owns it), so `hird run` translates Ctrl-C
%% into SIGTERM for the node. The default OTP signal handler is replaced
%% for the node's lifetime; the signals it halts on keep that behaviour.
-module(hird_stand).
-behaviour(gen_event).

-export([await/0]).
-export([init/1, handle_event/2, handle_call/2, handle_info/2, terminate/2]).

%% Blocks until SIGTERM, then stops the caller's supervisors and returns.
-spec await() -> ok.
await() ->
    ok = os:set_signal(sigterm, handle),
    ok = gen_event:swap_handler(erl_signal_server,
                                {erl_signal_handler, []},
                                {?MODULE, self()}),
    receive
        {?MODULE, shutdown} -> ok
    end,
    lists:foreach(fun stop_supervisor/1, supervisors(self())).

%% The supervisors `Parent` started, most recent first.
-spec supervisors(pid()) -> [pid()].
supervisors(Parent) ->
    {links, Links} = process_info(Parent, links),
    Sups = [P || P <- Links, is_pid(P), is_supervisor(P)],
    lists:reverse(lists:sort(Sups)).

is_supervisor(Pid) ->
    case proc_lib:initial_call(Pid) of
        {supervisor, _, _} -> true;
        _ -> false
    end.

%% Shuts `Sup` down as its parent and waits for it to exit. The link is
%% dropped first so the exit does not propagate back to the caller.
stop_supervisor(Sup) ->
    Ref = monitor(process, Sup),
    unlink(Sup),
    exit(Sup, shutdown),
    receive
        {'DOWN', Ref, process, Sup, _} -> ok
    end.

%% gen_event callbacks ----------------------------------------------------

%% @private The state is the standing process; the second element is the
%% replaced handler's terminate result, unused.
init({Standing, _}) ->
    {ok, Standing}.

%% @private
handle_event(sigterm, Standing) ->
    Standing ! {?MODULE, shutdown},
    {ok, Standing};
handle_event(sigquit, Standing) ->
    erlang:halt(),
    {ok, Standing};
handle_event(sigusr1, Standing) ->
    erlang:halt("Received SIGUSR1"),
    {ok, Standing};
handle_event(_Signal, Standing) ->
    {ok, Standing}.

%% @private
handle_call(_Request, Standing) ->
    {ok, ok, Standing}.

%% @private
handle_info(_Info, Standing) ->
    {ok, Standing}.

%% @private
terminate(_Args, _Standing) ->
    ok.

%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Standing mode: keeps a program up after its entry function has done its
%% setup. await/0 blocks the calling process until a stop trigger fires,
%% then shuts down every supervisor the caller started and returns, so the
%% caller's own teardown (the boot module's audit sync) runs after the
%% trees are gone.
%%
%% Two triggers exist, and a node arms every one that applies:
%%
%% - SIGTERM, where the platform has it (not Windows): the deployment
%%   signal — what an init system sends to stop a service. The default OTP
%%   signal handler is replaced for the node's lifetime; the signals it
%%   halts on keep that behaviour.
%% - End of file on standard input, when the launcher asked for it with
%%   `-hird_stop stdin`. `hird run` keeps the emulator's stdin as a pipe it
%%   owns and closes it on Ctrl-C or termination, on every platform; the
%%   pipe also closes if the launcher itself dies. A node started any other
%%   way never reads stdin, so a redirected or absent stdin cannot stop it.
%%
%% Supervisors started from the calling process are its linked children
%% whose initial call is `supervisor` — exactly the trees `supervise`
%% starts, since it calls start_link/0 in the caller. They are stopped in
%% reverse start order by an exit signal from their parent, which is the
%% OTP shutdown protocol: each supervisor terminates its children within
%% their shutdown timeouts before exiting itself.
-module(hird_stand).
-behaviour(gen_event).

-export([await/0, await/1, triggers/0]).
-export([init/1, handle_event/2, handle_call/2, handle_info/2, terminate/2]).

-export_type([trigger/0]).

%% A stop trigger: the SIGTERM signal, or end of file on an io device.
-type trigger() :: sigterm | {eof, io:device()}.

%% Blocks until a trigger fires, then stops the caller's supervisors and
%% returns. Arms the triggers that apply to this node (see triggers/0).
-spec await() -> ok.
await() ->
    await(triggers()).

%% await/0 over an explicit trigger list.
-spec await([trigger()]) -> ok.
await(Triggers) ->
    Standing = self(),
    lists:foreach(fun(Trigger) -> arm(Trigger, Standing) end, Triggers),
    receive
        {?MODULE, shutdown} -> ok
    end,
    lists:foreach(fun stop_supervisor/1, supervisors(Standing)).

%% The triggers that apply to this node: SIGTERM off Windows, and stdin
%% end of file when the launcher passed `-hird_stop stdin`.
-spec triggers() -> [trigger()].
triggers() ->
    Signal = case os:type() of
        {win32, _} -> [];
        _ -> [sigterm]
    end,
    Stdin = case init:get_argument(hird_stop) of
        {ok, [["stdin"]]} -> [{eof, standard_io}];
        _ -> []
    end,
    Signal ++ Stdin.

%% Arms one trigger to send `Standing` the shutdown message.
-spec arm(trigger(), pid()) -> ok.
arm(sigterm, Standing) ->
    ok = os:set_signal(sigterm, handle),
    ok = gen_event:swap_handler(erl_signal_server,
                                {erl_signal_handler, []},
                                {?MODULE, Standing});
arm({eof, Device}, Standing) ->
    _ = spawn(fun() -> drain(Device, Standing) end),
    ok.

%% Reads `Device` to its end, then tells `Standing` to shut down. Input
%% before the end is not a protocol: it is discarded.
drain(Device, Standing) ->
    case io:get_line(Device, "") of
        eof -> Standing ! {?MODULE, shutdown};
        {error, _} -> Standing ! {?MODULE, shutdown};
        _ -> drain(Device, Standing)
    end.

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

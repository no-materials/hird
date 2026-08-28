%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% The clock capability. A Hirð program reaches real time only through a
%% `Clock` value: `clock()` lowers to real/0, and `schedule(clock, pid, msg,
%% delay_ms)` to schedule/4, which delivers the message into the actor's
%% cast path after the delay. The value carries its implementation, so a
%% clock that is handed in can later be something other than real time
%% without the generated code changing.
%%
%% A scheduled message is not cancellable and outlives nothing: a message
%% whose destination has exited (a restarted actor has a new pid) is
%% dropped by the runtime, so an actor that ticks itself schedules its
%% first tick again from `init`.
-module(hird_clock).

-export([real/0, schedule/4]).

-export_type([clock/0]).

-opaque clock() :: {?MODULE, real}.

%% The real-time clock.
-spec real() -> clock().
real() ->
    {?MODULE, real}.

%% Delivers `Msg` to `Pid` as a gen_server cast after `DelayMs`
%% milliseconds. A negative delay is a bug, and crashes.
-spec schedule(clock(), pid(), term(), non_neg_integer()) -> ok.
schedule({?MODULE, real}, Pid, Msg, DelayMs)
  when is_pid(Pid), is_integer(DelayMs), DelayMs >= 0 ->
    _ = erlang:send_after(DelayMs, Pid, {'$gen_cast', Msg}),
    ok.

%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% The tool effect dispatcher. Every tool call site in generated code emits
%% `hird_tool_dispatch:call(ToolName, Caller, Handlers, Args)` — never a
%% direct handler invocation — so audit capture is unconditional: a mocked
%% call produces the same invocation record a real one does.
-module(hird_tool_dispatch).

-export([call/4]).

%% Dispatches one tool call. The handler is `{tool, ToolName}` in the
%% threaded handler map, falling back to the process-independent default
%% registry (hird_handlers) on a miss; a miss in both raises
%% `{unhandled_tool, ToolName}` — a crash for the supervisor, never a value
%% Hirð code sees. Around the invocation the dispatcher captures the
%% invocation record (tool, args, result, timestamp, caller) and sends it to
%% the audit sink (hird_audit); the record is dropped there when no sink is
%% running. `Caller` is the codegen-supplied caller id
%% (`<<"Module.function">>` or the actor form).
%%
%% A handler signals a domain failure by throwing `{hird_exn, Error}`,
%% where `Error` is a value of the tool's declared error type. The
%% dispatcher records the invocation with an `{err, Error}` result and
%% rethrows, so the failure propagates exactly as it would unaudited. Any
%% other exception is a crash, not a domain error: it propagates untouched
%% and unrecorded.
-spec call(atom(), binary(), #{term() => fun()}, term()) -> term().
call(ToolName, Caller, Handlers, Args) ->
    Handler = resolve(ToolName, Handlers),
    try Handler(Args, Handlers) of
        Result ->
            audit(ToolName, Caller, Args, {ok, Result}),
            Result
    catch
        throw:{hird_exn, Error}:Stacktrace ->
            audit(ToolName, Caller, Args, {err, Error}),
            erlang:raise(throw, {hird_exn, Error}, Stacktrace)
    end.

%% Sends one invocation record to the audit sink.
audit(ToolName, Caller, Args, Result) ->
    hird_audit:log(#{
        tool => ToolName,
        args => Args,
        result => Result,
        timestamp => erlang:system_time(millisecond),
        caller => Caller
    }).

%% The handler for `ToolName`: threaded map first, then the registry.
resolve(ToolName, Handlers) ->
    case maps:find({tool, ToolName}, Handlers) of
        {ok, Handler} ->
            Handler;
        error ->
            case hird_handlers:lookup_handler({tool, ToolName}) of
                {ok, Handler} -> Handler;
                error -> erlang:error({unhandled_tool, ToolName})
            end
    end.

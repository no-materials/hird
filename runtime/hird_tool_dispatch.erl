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
-spec call(atom(), binary(), #{term() => fun()}, term()) -> term().
call(ToolName, Caller, Handlers, Args) ->
    Handler = resolve(ToolName, Handlers),
    Result = Handler(Args, Handlers),
    hird_audit:log(#{
        tool => ToolName,
        args => Args,
        result => {ok, Result},
        timestamp => erlang:system_time(millisecond),
        caller => Caller
    }),
    Result.

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

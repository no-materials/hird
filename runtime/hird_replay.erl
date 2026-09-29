%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% The replay cursor: a gen_server holding a recorded audit log, matched
%% strict-sequentially against the program's tool dispatches. Every line is
%% validated at startup and decoded type-directedly then, except a generic
%% tool's record: its shapes depend on the type arguments its call fixes,
%% so it is decoded when that call is offered. While the cursor is running
%% the dispatcher routes every tool call here instead of resolving a
%% handler, so no `handle` or `install` block in the program can shadow the
%% log. Each offer must name the tool and args of the record at the cursor,
%% which then yields its logged result — failures included; any mismatch
%% is a divergence the dispatcher raises as a crash, never a value Hirð
%% code sees.
-module(hird_replay).
-behaviour(gen_server).

-export([start_link/2, active/0, offer/3, finish/0]).
-export([init/1, handle_call/3, handle_cast/2]).

%% Where and how a replay diverged: the 0-based position and total log
%% size, what the log holds there (absent when exhausted; without `args`
%% when they do not decode, as a generic tool's record under an offer that
%% does not supply its type arguments), and what the program offered.
-type divergence() :: #{
    kind := log_exhausted | tool_mismatch | args_mismatch,
    position := non_neg_integer(),
    log_size := non_neg_integer(),
    expected => #{tool := atom(), args => term()},
    offered := #{tool := atom(), args := term()}
}.

-export_type([divergence/0]).

%% Starts the cursor registered as ?MODULE, loading every line of the log
%% at `Path` against the merged signature `Tables`. Fails with
%% `{replay_load_error, …}` when the file is unreadable or any line does
%% not load: a malformed envelope, another schema version, a tool the
%% program does not declare, or — for a non-generic tool — values that do
%% not decode.
-spec start_link(file:filename_all(), [hird_types:table()]) ->
    {ok, pid()} | {error, term()}.
start_link(Path, Tables) ->
    gen_server:start_link({local, ?MODULE}, ?MODULE, {Path, Tables}, []).

%% Whether a replay cursor is running (the dispatcher's routing test).
-spec active() -> boolean().
active() ->
    whereis(?MODULE) =/= undefined.

%% Offers the program's next tool call, a generic tool's type arguments
%% included. A match yields the logged result — `{ok, Value}` or
%% `{err, Error}` — and advances the cursor; a mismatch yields
%% `{diverged, Divergence}` without advancing.
-spec offer(atom(), [hird_types:shape()], term()) ->
    {ok, term()} | {err, term()} | {diverged, divergence()}.
offer(Tool, TypeArgs, Args) ->
    gen_server:call(?MODULE, {offer, Tool, TypeArgs, Args}, infinity).

%% Checks the run consumed the whole log: `ok` when nothing remains,
%% `{error, {replay_incomplete, …}}` otherwise — a truncated replay is
%% not a faithful one.
-spec finish() -> ok | {error, term()}.
finish() ->
    gen_server:call(?MODULE, finish, infinity).

%% gen_server callbacks ---------------------------------------------------

%% @private
init({Path, Tables}) ->
    Table = merge(Tables),
    case file:read_file(Path) of
        {ok, Bytes} ->
            try load(lines(Bytes), Table, 1) of
                Log ->
                    {ok, #{log => Log, position => 0, size => length(Log),
                           table => Table}}
            catch
                error:{replay_load_error, _} = Reason ->
                    {stop, Reason}
            end;
        {error, Reason} ->
            {stop, {replay_load_error, #{file => Path, reason => Reason}}}
    end.

%% @private
handle_call({offer, Tool, TypeArgs, Args}, _From,
            #{log := Log, position := P, table := Table} = State) ->
    case Log of
        [Entry | Rest] ->
            case recorded(Entry, Tool, TypeArgs, Table) of
                #{tool := Tool, args := Logged, result := Result}
                        when Logged == Args ->
                    {reply, Result, State#{log := Rest, position := P + 1}};
                Expected ->
                    {reply, {diverged, divergence(Expected, Tool, Args, State)},
                     State}
            end;
        [] ->
            {reply, {diverged, divergence(none, Tool, Args, State)}, State}
    end;
handle_call(finish, _From, #{log := Log, position := P, size := Size} = State) ->
    Reply = case Log of
        [] -> ok;
        _ -> {error, {replay_incomplete, #{consumed => P, log_size => Size}}}
    end,
    {reply, Reply, State}.

%% @private
handle_cast(_Msg, State) ->
    {noreply, State}.

%% The record at the cursor, as far as the offer lets it decode: a loaded
%% record in full; a generic tool's line decoded at the offered type
%% arguments when the offer names its tool; otherwise the recorded tool
%% alone.
recorded({decoded, Record}, _Tool, _TypeArgs, _Table) ->
    Record;
recorded({pending, Tool, Line}, Tool, TypeArgs, Table) ->
    try
        hird_types:decode_invocation(Line, TypeArgs, Table)
    catch
        error:{decode_error, _} -> #{tool => Tool}
    end;
recorded({pending, Logged, _Line}, _Tool, _TypeArgs, _Table) ->
    #{tool => Logged}.

%% The divergence for an offer the record at the cursor (`none` when the
%% log is exhausted) does not match.
divergence(Expected, Tool, Args, #{position := P, size := Size}) ->
    Base = #{position => P, log_size => Size,
             offered => #{tool => Tool, args => Args}},
    case Expected of
        none ->
            Base#{kind => log_exhausted};
        #{tool := Logged} ->
            Kind = case Logged =:= Tool of
                true -> args_mismatch;
                false -> tool_mismatch
            end,
            Base#{kind => Kind, expected => maps:with([tool, args], Expected)}
    end.

%% The log's lines, without the trailing empty split a final newline
%% leaves. Blank lines elsewhere fail loading, as they should.
lines(Bytes) ->
    case lists:reverse(binary:split(Bytes, <<"\n">>, [global])) of
        [<<>> | Rest] -> lists:reverse(Rest);
        All -> lists:reverse(All)
    end.

%% Every line loaded, failures tagged with their 1-based line number.
load([], _Table, _N) ->
    [];
load([Line | Rest], Table, N) ->
    Entry = try
        entry(Line, Table)
    catch
        error:Reason ->
            erlang:error({replay_load_error, #{line => N, reason => Reason}})
    end,
    [Entry | load(Rest, Table, N + 1)].

%% One line's log entry: decoded now unless its tool is generic, whose
%% line waits for the offer that supplies its type arguments.
entry(Line, #{tools := Tools} = Table) ->
    Tool = hird_types:decode_tool(Line, Table),
    case maps:get(Tool, Tools) of
        #{params := 0} -> {decoded, hird_types:decode_invocation(Line, [], Table)};
        #{} -> {pending, Tool, Line}
    end.

%% Merges signature tables the way the audit sink does.
merge(Tables) ->
    lists:foldl(
        fun(#{tools := Tools, types := Types}, #{tools := AccT, types := AccY}) ->
            #{tools => maps:merge(AccT, Tools),
              types => maps:merge(AccY, Types)}
        end,
        #{tools => #{}, types => #{}},
        Tables).

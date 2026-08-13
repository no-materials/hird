%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% The replay cursor: a gen_server holding a recorded audit log, decoded
%% type-directedly at startup and matched strict-sequentially against the
%% program's tool dispatches. While the cursor is running the dispatcher
%% routes every tool call here instead of resolving a handler, so no
%% `handle` or `install` block in the program can shadow the log. Each
%% offer must name the tool and args of the record at the cursor, which
%% then yields its logged result — failures included; any mismatch is a
%% divergence the dispatcher raises as a crash, never a value Hirð code
%% sees.
-module(hird_replay).
-behaviour(gen_server).

-export([start_link/2, active/0, offer/2, finish/0]).
-export([init/1, handle_call/3, handle_cast/2]).

%% Where and how a replay diverged: the 0-based position and total log
%% size, what the log holds there (absent when exhausted), and what the
%% program offered.
-type divergence() :: #{
    kind := log_exhausted | tool_mismatch | args_mismatch,
    position := non_neg_integer(),
    log_size := non_neg_integer(),
    expected => #{tool := atom(), args := term()},
    offered := #{tool := atom(), args := term()}
}.

-export_type([divergence/0]).

%% Starts the cursor registered as ?MODULE, decoding every line of the
%% log at `Path` against the merged signature `Tables`. Fails with
%% `{replay_load_error, …}` when the file is unreadable or any line does
%% not decode.
-spec start_link(file:filename_all(), [hird_types:table()]) ->
    {ok, pid()} | {error, term()}.
start_link(Path, Tables) ->
    gen_server:start_link({local, ?MODULE}, ?MODULE, {Path, Tables}, []).

%% Whether a replay cursor is running (the dispatcher's routing test).
-spec active() -> boolean().
active() ->
    whereis(?MODULE) =/= undefined.

%% Offers the program's next tool call. A match yields the logged result —
%% `{ok, Value}` or `{err, Error}` — and advances the cursor; a mismatch
%% yields `{diverged, Divergence}` without advancing.
-spec offer(atom(), term()) ->
    {ok, term()} | {err, term()} | {diverged, divergence()}.
offer(Tool, Args) ->
    gen_server:call(?MODULE, {offer, Tool, Args}, infinity).

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
            try decode_lines(lines(Bytes), Table, 1) of
                Log ->
                    {ok, #{log => Log, position => 0, size => length(Log)}}
            catch
                error:{replay_load_error, _} = Reason ->
                    {stop, Reason}
            end;
        {error, Reason} ->
            {stop, {replay_load_error, #{file => Path, reason => Reason}}}
    end.

%% @private
handle_call({offer, Tool, Args}, _From, #{log := Log, position := P} = State) ->
    case Log of
        [#{tool := Tool, args := Logged, result := Result} | Rest]
                when Logged == Args ->
            {reply, Result, State#{log := Rest, position := P + 1}};
        _ ->
            {reply, {diverged, divergence(Log, Tool, Args, State)}, State}
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

%% The divergence for an offer the record at the cursor does not match.
divergence(Log, Tool, Args, #{position := P, size := Size}) ->
    Base = #{position => P, log_size => Size,
             offered => #{tool => Tool, args => Args}},
    case Log of
        [] ->
            Base#{kind => log_exhausted};
        [#{tool := Logged, args := LoggedArgs} | _] ->
            Kind = case Logged =:= Tool of
                true -> args_mismatch;
                false -> tool_mismatch
            end,
            Base#{kind => Kind,
                  expected => #{tool => Logged, args => LoggedArgs}}
    end.

%% The log's lines, without the trailing empty split a final newline
%% leaves. Blank lines elsewhere fail decoding, as they should.
lines(Bytes) ->
    case lists:reverse(binary:split(Bytes, <<"\n">>, [global])) of
        [<<>> | Rest] -> lists:reverse(Rest);
        All -> lists:reverse(All)
    end.

%% Every line decoded, failures tagged with their 1-based line number.
decode_lines([], _Table, _N) ->
    [];
decode_lines([Line | Rest], Table, N) ->
    Record = try
        hird_types:decode_invocation(Line, Table)
    catch
        error:Reason ->
            erlang:error({replay_load_error, #{line => N, reason => Reason}})
    end,
    [Record | decode_lines(Rest, Table, N + 1)].

%% Merges signature tables the way the audit sink does.
merge(Tables) ->
    lists:foldl(
        fun(#{tools := Tools, types := Types}, #{tools := AccT, types := AccY}) ->
            #{tools => maps:merge(AccT, Tools),
              types => maps:merge(AccY, Types)}
        end,
        #{tools => #{}, types => #{}},
        Tables).

%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% The audit log sink: a gen_server that accepts invocation records from the
%% tool dispatcher and writes them as canonical JSON lines (one record per
%% line, ordered by arrival). Encoding is type-directed against the
%% signature table generated base modules expose as `hird_tools@/0`; startup
%% wiring registers it with register_tools/1.
-module(hird_audit).
-behaviour(gen_server).

-export([start_link/1, register_tools/1, log/1, sync/0]).
-export([init/1, handle_call/3, handle_cast/2, terminate/2]).

-type sink() :: stdout | {file, file:filename_all()}.
-type option() :: {sink, sink()} | {tools, hird_types:table()}.

-export_type([sink/0, option/0]).

%% The empty signature table.
-define(NO_TOOLS, #{tools => #{}, types => #{}}).

%% Starts the sink registered as ?MODULE. Options: `{sink, stdout}` (the
%% default) or `{sink, {file, Path}}` (opened for append — an audit log is
%% never truncated by a restart), and optionally `{tools, Table}`.
-spec start_link([option()]) -> {ok, pid()} | {error, term()}.
start_link(Options) ->
    gen_server:start_link({local, ?MODULE}, ?MODULE, Options, []).

%% Merges a generated module's signature table into the sink's table.
-spec register_tools(hird_types:table()) -> ok.
register_tools(Table) ->
    gen_server:call(?MODULE, {register_tools, Table}).

%% Records one invocation. Asynchronous; a no-op when no sink is running,
%% so dispatch works (unaudited) without one.
-spec log(hird_types:record_in()) -> ok.
log(Record) ->
    gen_server:cast(?MODULE, {log, Record}).

%% Blocks until every previously logged record has been written.
-spec sync() -> ok.
sync() ->
    gen_server:call(?MODULE, sync).

%% gen_server callbacks ---------------------------------------------------

%% @private
init(Options) ->
    Tools = proplists:get_value(tools, Options, ?NO_TOOLS),
    case proplists:get_value(sink, Options, stdout) of
        stdout ->
            {ok, #{device => standard_io, tools => Tools}};
        {file, Path} ->
            case file:open(Path, [append, {encoding, utf8}]) of
                {ok, Device} -> {ok, #{device => Device, tools => Tools}};
                {error, Reason} -> {stop, {cannot_open_sink, Path, Reason}}
            end
    end.

%% @private
handle_call({register_tools, Table}, _From, #{tools := Tools} = State) ->
    Merged = #{
        tools => maps:merge(maps:get(tools, Tools), maps:get(tools, Table)),
        types => maps:merge(maps:get(types, Tools), maps:get(types, Table))
    },
    {reply, ok, State#{tools := Merged}};
handle_call(sync, _From, State) ->
    {reply, ok, State}.

%% @private
handle_cast({log, Record}, #{device := Device, tools := Tools} = State) ->
    Line = hird_types:encode_invocation(Record, Tools),
    ok = io:put_chars(Device, [Line, $\n]),
    {noreply, State}.

%% @private
terminate(_Reason, #{device := standard_io}) ->
    ok;
terminate(_Reason, #{device := Device}) ->
    _ = file:close(Device),
    ok.

%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% The audit log sink: a gen_server that writes invocation records as
%% canonical JSON lines (one record per line, ordered by arrival). Encoding
%% is type-directed against the signature table generated base modules
%% expose as `hird_tools@/0`; startup wiring registers it with
%% register_tools/1, and the sink publishes the merged table as a
%% persistent term, so log/1 encodes in the calling process and the sink
%% only appends finished lines, a batch per write.
-module(hird_audit).
-behaviour(gen_server).

-export([start_link/1, register_tools/1, log/1, sync/0]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2, terminate/2]).

-type sink() :: stdout | {file, file:filename_all()}.
-type option() :: {sink, sink()} | {tools, hird_types:table()}.

-export_type([sink/0, option/0]).

%% The empty signature table.
-define(NO_TOOLS, #{tools => #{}, types => #{}}).

%% The persistent term a running sink publishes for log/1: its merged
%% signature table and its backlog counter.
-define(KEY, {?MODULE, sink}).

%% Bytes of logged lines the sink may have queued before log/1 waits for
%% it instead of casting.
-define(BACKLOG, 8 bsl 20).

%% Bytes the sink buffers before writing them out.
-define(BATCH, 64 bsl 10).

%% Milliseconds a buffered line waits at most before it is written.
-define(DELAY, 10).

%% Starts the sink registered as ?MODULE. Options: `{sink, stdout}` (the
%% default) or `{sink, {file, Path}}` (opened for append — an audit log is
%% never truncated by a restart), and optionally `{tools, Table}`.
-spec start_link([option()]) -> {ok, pid()} | {error, term()}.
start_link(Options) ->
    gen_server:start_link({local, ?MODULE}, ?MODULE, Options,
                          [{spawn_opt, [{message_queue_data, off_heap}]}]).

%% Merges a generated module's signature table into the sink's table.
%% Republishes the persistent term, which scans every process: a startup
%% call, not a per-record one.
-spec register_tools(hird_types:table()) -> ok.
register_tools(Table) ->
    gen_server:call(?MODULE, {register_tools, Table}).

%% Records one invocation, encoded in the calling process: fails as
%% hird_types:encode_invocation/2 does when the registered tables cannot
%% encode it. Asynchronous while the sink has under ?BACKLOG bytes queued;
%% past that, waits until the sink takes the line. A no-op when no sink is
%% running, so dispatch works (unaudited) without one.
-spec log(hird_types:record_in()) -> ok.
log(Record) ->
    case whereis(?MODULE) of
        undefined -> ok;
        Sink -> log(Sink, persistent_term:get(?KEY, undefined), Record)
    end.

%% `undefined`: the sink is registered but has not published yet.
log(_Sink, undefined, _Record) ->
    ok;
log(Sink, #{tools := Tools, backlog := Backlog}, Record) ->
    Line = hird_types:encode_invocation(Record, Tools),
    case atomics:add_get(Backlog, 1, byte_size(Line)) =< ?BACKLOG of
        true -> gen_server:cast(Sink, {log, Line});
        false -> wait(Sink, Line)
    end.

%% Hands `Line` to the sink and waits until it is taken. A sink that exits
%% meanwhile drops it, as it would a cast.
wait(Sink, Line) ->
    try gen_server:call(Sink, {log, Line}, infinity)
    catch exit:_ -> ok
    end.

%% Blocks until every previously logged record has been written, however
%% long the backlog takes to drain.
-spec sync() -> ok.
sync() ->
    gen_server:call(?MODULE, sync, infinity).

%% gen_server callbacks ---------------------------------------------------
%%
%% The sink buffers lines and writes them a batch at a time: once ?BATCH
%% bytes are buffered, ?DELAY ms after the first of them, on sync, and on
%% termination.

%% @private
init(Options) ->
    Tools = proplists:get_value(tools, Options, ?NO_TOOLS),
    case proplists:get_value(sink, Options, stdout) of
        stdout ->
            {ok, sink(standard_io, Tools)};
        {file, Path} ->
            case file:open(Path, [append, raw, binary]) of
                {ok, Fd} -> {ok, sink(Fd, Tools)};
                {error, Reason} -> {stop, {cannot_open_sink, Path, Reason}}
            end
    end.

%% A fresh sink's state on `Device`, published.
sink(Device, Tools) ->
    publish(#{device => Device, tools => Tools, backlog => atomics:new(1, []),
              buffer => [], buffered => 0, timer => none}).

%% Publishes the table and backlog counter for log/1.
publish(#{tools := Tools, backlog := Backlog} = State) ->
    persistent_term:put(?KEY, #{tools => Tools, backlog => Backlog}),
    State.

%% @private
handle_call({log, Line}, _From, State) ->
    {reply, ok, take(Line, State)};
handle_call({register_tools, Table}, _From, #{tools := Tools} = State) ->
    Merged = #{
        tools => maps:merge(maps:get(tools, Tools), maps:get(tools, Table)),
        types => maps:merge(maps:get(types, Tools), maps:get(types, Table))
    },
    {reply, ok, publish(State#{tools := Merged})};
handle_call(sync, _From, State) ->
    {reply, ok, flush(State)}.

%% @private
handle_cast({log, Line}, State) ->
    {noreply, take(Line, State)}.

%% @private
handle_info({timeout, Timer, flush}, #{timer := Timer} = State) ->
    {noreply, flush(State)};
%% A timer whose batch is already written, or a stray message.
handle_info(_Stale, State) ->
    {noreply, State}.

%% @private
terminate(_Reason, #{device := Device} = State) ->
    _ = persistent_term:erase(?KEY),
    _ = flush(State),
    case Device of
        standard_io -> ok;
        Fd -> _ = file:close(Fd), ok
    end.

%% Buffers `Line` and releases its bytes from the backlog, writing once
%% ?BATCH bytes are buffered; the first line of a batch starts its timer.
take(Line, #{backlog := Backlog, buffer := Buffer, buffered := Buffered}
             = State) ->
    Size = byte_size(Line),
    ok = atomics:sub(Backlog, 1, Size),
    Taken = State#{buffer := [Buffer, Line, $\n],
                   buffered := Buffered + Size + 1},
    if
        Buffered + Size + 1 >= ?BATCH -> flush(Taken);
        Buffered =:= 0 ->
            Taken#{timer := erlang:start_timer(?DELAY, self(), flush)};
        true -> Taken
    end.

%% Writes the buffered lines. A timer still running for them is left to
%% fire unmatched.
flush(#{buffered := 0} = State) ->
    State;
flush(#{device := Device, buffer := Buffer} = State) ->
    ok = write(Device, Buffer),
    State#{buffer := [], buffered := 0, timer := none}.

%% Hands `Data` to the sink's device.
write(standard_io, Data) -> io:put_chars(standard_io, Data);
write(Fd, Data) -> file:write(Fd, Data).

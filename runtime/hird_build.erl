%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% The build driver: compiles the modules of a build that missed the build
%% cache inside the emulator that goes on to run them, so `hird run` boots
%% one emulator rather than one to compile and one to run. `hird build`
%% uses the same driver and halts instead of running.
%%
%% The launcher passes the work as init arguments:
%%
%% - `-hird_out Dir`: the build directory. Each module compiles from
%%   `Dir/Module.erl` to `Dir/Module.beam`.
%% - `-hird_compile Module ...`: the modules to compile.
%% - `-hird_cache Dir Key ...`: the build cache, when there is one, and one
%%   entry key per module, in `-hird_compile` order. A compiled module is
%%   stored as `Dir/Key.beam`, written aside and renamed into place, so a
%%   concurrent build never reads half an entry; an entry already there is
%%   left alone.
%%
%% Modules compile in parallel, on one worker per scheduler. The compiler
%% reports to the group leader, which under `hird run` is the program's
%% standard output, so each compile reports into a buffer instead; the
%% buffers go to standard error in module order once every module is done.
-module(hird_build).

-export([build/0, run/0, compile/2]).

-export_type([job/0]).

%% One module to compile, and the cache entry to store it as, if any.
-type job() :: {Module :: string(), {CacheDir :: string(), Key :: string()} | none}.

%% Entry point for `erl -s hird_build build`: compiles, then halts with 0,
%% or 1 if a module failed to compile.
-spec build() -> no_return().
build() ->
    case compile_arguments() of
        ok -> halt(0);
        error -> halt(1)
    end.

%% Entry point for `erl -s hird_build run`: compiles, then runs the program
%% through the boot module, which halts; halts with 1 instead if a module
%% failed to compile.
-spec run() -> no_return().
run() ->
    case compile_arguments() of
        ok -> hird_boot:run();
        error -> halt(1)
    end.

%% compile/2 over the init arguments. A crash is reported here, as the boot
%% module reports one: left to init, it would go to standard output and
%% leave a crash dump in the working directory.
-spec compile_arguments() -> ok | error.
compile_arguments() ->
    try
        {ok, [[Out]]} = init:get_argument(hird_out),
        {ok, [Modules]} = init:get_argument(hird_compile),
        Entries = case init:get_argument(hird_cache) of
            {ok, [[Dir | Keys]]} -> [{Dir, Key} || Key <- Keys];
            error -> [none || _ <- Modules]
        end,
        {Status, Reports} = compile(Out, lists:zip(Modules, Entries)),
        ok = io:put_chars(standard_error, Reports),
        Status
    catch
        Class:Reason:Stack ->
            io:format(standard_error, "hird: build driver error: ~p~n",
                      [{Class, Reason, Stack}]),
            error
    end.

%% Compiles each job's `Out/Module.erl` to `Out/Module.beam` and stores it
%% in the cache. Returns `error` if any module failed to compile, with the
%% compiler's reports in job order.
-spec compile(file:filename(), [job()]) -> {ok | error, [binary()]}.
compile(Out, Jobs) ->
    Results = parallel(fun(Job) -> compile_one(Out, Job) end, Jobs),
    Status = case lists:all(fun({Compiled, _}) -> Compiled =:= ok end, Results) of
        true -> ok;
        false -> error
    end,
    {Status, [Report || {_, Report} <- Results]}.

%% Compiles one job, returning whether it compiled and what the compiler
%% reported.
-spec compile_one(file:filename(), job()) -> {ok | error, binary()}.
compile_one(Out, {Module, Entry}) ->
    Reports = spawn_link(fun() -> collect([]) end),
    group_leader(Reports, self()),
    Result = compile:file(filename:join(Out, Module),
                          [binary, deterministic, report]),
    Reports ! {take, self()},
    Report = receive {Reports, Chars} -> Chars end,
    case Result of
        {ok, _, Beam} ->
            ok = file:write_file(filename:join(Out, Module ++ ".beam"), Beam),
            store(Entry, Beam),
            {ok, Report};
        error ->
            {error, Report}
    end.

%% Stores `Beam` as its cache entry, best effort.
-spec store({string(), string()} | none, binary()) -> ok.
store(none, _Beam) ->
    ok;
store({Dir, Key}, Beam) ->
    Entry = filename:join(Dir, Key ++ ".beam"),
    case filelib:is_regular(Entry) of
        true ->
            ok;
        false ->
            Staged = filename:join(Dir, Key ++ "." ++ os:getpid() ++ ".tmp"),
            case file:write_file(Staged, Beam) =:= ok
                andalso file:rename(Staged, Entry) =:= ok of
                true -> ok;
                false -> _ = file:delete(Staged), ok
            end
    end.

%% Applies `F` to each element of `List` on up to one worker per scheduler,
%% returning the results in `List` order. A crashing worker crashes the
%% caller.
-spec parallel(fun((A) -> B), [A]) -> [B].
parallel(F, List) ->
    Work = list_to_tuple(List),
    Next = atomics:new(1, []),
    Count = min(erlang:system_info(schedulers_online), tuple_size(Work)),
    Workers = [spawn_monitor(fun() -> exit({done, work(F, Work, Next, [])}) end)
               || _ <- lists:seq(1, Count)],
    Done = lists:append(
        [receive {'DOWN', Ref, process, _, Reason} -> {done, Results} = Reason,
                                                      Results
         end || {_, Ref} <- Workers]),
    [Result || {_, Result} <- lists:sort(Done)].

%% Takes the next unclaimed index of `Work` until none is left, returning
%% each result paired with its index.
work(F, Work, Next, Done) ->
    case atomics:add_get(Next, 1, 1) of
        I when I > tuple_size(Work) -> Done;
        I -> work(F, Work, Next, [{I, F(element(I, Work))} | Done])
    end.

%% An I/O server that keeps what is written to it until asked for it.
collect(Chars) ->
    receive
        {io_request, From, ReplyAs, Request} ->
            {Reply, More} = request(Request),
            From ! {io_reply, ReplyAs, Reply},
            collect([Chars | More]);
        {take, From} ->
            From ! {self(), iolist_to_binary(Chars)}
    end.

%% Executes an output request as UTF-8; no other request is supported.
request({put_chars, Encoding, Chars}) ->
    {ok, unicode:characters_to_binary(Chars, Encoding)};
request({put_chars, Encoding, Module, Function, Args}) ->
    request({put_chars, Encoding, apply(Module, Function, Args)});
request(_) ->
    {{error, request}, []}.

%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% compile/2 over scratch build and cache directories under _build: what
%% it writes to each, what it leaves alone, and the reports it returns.
-module(hird_build_tests).

-include_lib("eunit/include/eunit.hrl").

%% A fresh scratch directory under _build, holding empty `out` and `cache`
%% directories, returned in that order.
scratch(Name) ->
    Dir = filename:join("_build", Name),
    _ = file:del_dir_r(Dir),
    [Out, Cache] = [filename:join(Dir, Sub) || Sub <- ["out", "cache"]],
    ok = filelib:ensure_path(Out),
    ok = filelib:ensure_path(Cache),
    {Out, Cache}.

%% Writes module `Name`, with `Body` after its module attribute, to `Out`.
source(Out, Name, Body) ->
    ok = file:write_file(filename:join(Out, Name ++ ".erl"),
                         ["-module(", Name, ").\n-export([f/0]).\n", Body]).

compiles_into_the_build_and_the_cache_test() ->
    {Out, Cache} = scratch("build_compiles"),
    source(Out, "hird_build_t1", "f() -> one.\n"),
    source(Out, "hird_build_t2", "f() -> two.\n"),
    ?assertEqual({ok, [<<>>, <<>>]},
                 hird_build:compile(Out, [{"hird_build_t1", {Cache, "k1"}},
                                          {"hird_build_t2", {Cache, "k2"}}])),
    {ok, Beam1} = file:read_file(filename:join(Out, "hird_build_t1.beam")),
    ?assertEqual({ok, Beam1}, file:read_file(filename:join(Cache, "k1.beam"))),
    {ok, Beam2} = file:read_file(filename:join(Out, "hird_build_t2.beam")),
    ?assertEqual({ok, Beam2}, file:read_file(filename:join(Cache, "k2.beam"))),
    {ok, Entries} = file:list_dir(Cache),
    ?assertEqual(["k1.beam", "k2.beam"], lists:sort(Entries)).

leaves_an_existing_entry_alone_test() ->
    {Out, Cache} = scratch("build_existing"),
    source(Out, "hird_build_t3", "f() -> three.\n"),
    Entry = filename:join(Cache, "k3.beam"),
    ok = file:write_file(Entry, <<"already here">>),
    ?assertEqual({ok, [<<>>]},
                 hird_build:compile(Out, [{"hird_build_t3", {Cache, "k3"}}])),
    ?assertEqual({ok, <<"already here">>}, file:read_file(Entry)),
    ?assert(filelib:is_regular(filename:join(Out, "hird_build_t3.beam"))).

compiles_without_a_cache_test() ->
    {Out, Cache} = scratch("build_uncached"),
    source(Out, "hird_build_t4", "f() -> four.\n"),
    ?assertEqual({ok, [<<>>]}, hird_build:compile(Out, [{"hird_build_t4", none}])),
    ?assert(filelib:is_regular(filename:join(Out, "hird_build_t4.beam"))),
    ?assertEqual({ok, []}, file:list_dir(Cache)).

reports_come_back_in_job_order_test() ->
    {Out, _} = scratch("build_reports"),
    Names = ["hird_build_t5", "hird_build_t6", "hird_build_t7"],
    source(Out, "hird_build_t5", "f() -> X = 1, ok.\n"),
    source(Out, "hird_build_t6", "f() -> ok.\n"),
    source(Out, "hird_build_t7", "f() -> Y = 1, ok.\n"),
    {ok, [Five, Six, Seven]} =
        hird_build:compile(Out, [{Name, none} || Name <- Names]),
    ?assertMatch({match, _},
                 re:run(Five, "hird_build_t5\\.erl:3:8: Warning: variable 'X' is unused")),
    ?assertEqual(<<>>, Six),
    ?assertMatch({match, _},
                 re:run(Seven, "hird_build_t7\\.erl:3:8: Warning: variable 'Y' is unused")).

a_failed_module_fails_the_build_and_is_not_cached_test() ->
    {Out, Cache} = scratch("build_fails"),
    source(Out, "hird_build_t8", "f() -> undefined_call().\n"),
    source(Out, "hird_build_t9", "f() -> nine.\n"),
    {Status, [Eight, Nine]} =
        hird_build:compile(Out, [{"hird_build_t8", {Cache, "k8"}},
                                 {"hird_build_t9", {Cache, "k9"}}]),
    ?assertEqual(error, Status),
    ?assertMatch({match, _},
                 re:run(Eight, "hird_build_t8\\.erl:3:8: function undefined_call/0 undefined")),
    ?assertEqual(<<>>, Nine),
    ?assertNot(filelib:is_regular(filename:join(Out, "hird_build_t8.beam"))),
    ?assertEqual({ok, ["k9.beam"]}, file:list_dir(Cache)).

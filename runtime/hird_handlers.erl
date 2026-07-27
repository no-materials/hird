%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% The process-independent default-handler registry. Lexical handle blocks
%% thread a handler map through calls; this registry is the fallback the
%% dispatcher consults on a map miss, and the only mechanism for supplying
%% handlers to spawned actors (handler maps never cross the spawn boundary),
%% so deployments and test harnesses install process-wide defaults here.
%%
%% Keys follow the threaded map's scheme: `{tool, Name}` for tool effects,
%% the bare head atom for other effects. Entries are the dispatcher's binary
%% funs `fun(Args, Handlers)`. Storage is persistent_term: installs are rare
%% (startup, test setup) and lookups are on every registry-resolved call.
-module(hird_handlers).

-export([install_handler/2, lookup_handler/1, with_handlers/2]).

-type key() :: atom() | {tool, atom()}.
-type handler() :: fun((term(), map()) -> term()).

-export_type([key/0, handler/0]).

%% Installs a default handler, replacing any previous entry for the key.
-spec install_handler(key(), handler()) -> ok.
install_handler(Key, Handler) when is_function(Handler, 2) ->
    persistent_term:put({?MODULE, Key}, Handler).

%% The installed handler for a key.
-spec lookup_handler(key()) -> {ok, handler()} | error.
lookup_handler(Key) ->
    case persistent_term:get({?MODULE, Key}, undefined) of
        undefined -> error;
        Handler -> {ok, Handler}
    end.

%% Runs `Fun` with the given handlers installed, restoring the previous
%% registry state afterwards (installs and erases survive a crash in `Fun`).
-spec with_handlers([{key(), handler()}], fun(() -> Result)) -> Result.
with_handlers(Handlers, Fun) ->
    Saved = [{Key, lookup_handler(Key)} || {Key, _} <- Handlers],
    lists:foreach(
        fun({Key, Handler}) -> install_handler(Key, Handler) end, Handlers),
    try
        Fun()
    after
        lists:foreach(fun restore/1, Saved)
    end.

%% Puts one key back to its pre-`with_handlers` state.
restore({Key, {ok, Handler}}) ->
    install_handler(Key, Handler);
restore({Key, error}) ->
    _ = persistent_term:erase({?MODULE, Key}),
    ok.

%% Copyright 2026 the Hird Authors
%% SPDX-License-Identifier: Apache-2.0 OR MIT
%%
%% Registry behaviour: install/lookup, and with_handlers scoping (install,
%% restore, restore-on-crash).
-module(hird_handlers_tests).

-include_lib("eunit/include/eunit.hrl").

install_then_lookup_test() ->
    Handler = fun(Args, _Handlers) -> Args end,
    ok = hird_handlers:install_handler({tool, install_case}, Handler),
    ?assertEqual({ok, Handler},
                 hird_handlers:lookup_handler({tool, install_case})).

lookup_misses_for_uninstalled_keys_test() ->
    ?assertEqual(error, hird_handlers:lookup_handler({tool, never_installed})).

bare_effect_keys_work_test() ->
    Handler = fun(_Args, _Handlers) -> logged end,
    ok = hird_handlers:install_handler(log_case, Handler),
    ?assertEqual({ok, Handler}, hird_handlers:lookup_handler(log_case)).

with_handlers_installs_for_the_scope_test() ->
    Result = hird_handlers:with_handlers(
        [{{tool, scoped}, fun(_Args, _Handlers) -> scoped end}],
        fun() ->
            {ok, Handler} = hird_handlers:lookup_handler({tool, scoped}),
            Handler(ok, #{})
        end),
    ?assertEqual(scoped, Result),
    ?assertEqual(error, hird_handlers:lookup_handler({tool, scoped})).

with_handlers_restores_previous_entries_test() ->
    Outer = fun(_Args, _Handlers) -> outer end,
    ok = hird_handlers:install_handler({tool, layered}, Outer),
    hird_handlers:with_handlers(
        [{{tool, layered}, fun(_Args, _Handlers) -> inner end}],
        fun() ->
            {ok, Handler} = hird_handlers:lookup_handler({tool, layered}),
            ?assertEqual(inner, Handler(ok, #{}))
        end),
    ?assertEqual({ok, Outer}, hird_handlers:lookup_handler({tool, layered})).

with_handlers_restores_after_a_crash_test() ->
    ?assertError(boom,
                 hird_handlers:with_handlers(
                     [{{tool, doomed}, fun(_Args, _Handlers) -> ok end}],
                     fun() -> erlang:error(boom) end)),
    ?assertEqual(error, hird_handlers:lookup_handler({tool, doomed})).

#!/usr/bin/env sh
# Compiles the runtime and its eunit suites, then runs every suite.
# Requires Erlang/OTP on PATH; run from anywhere.
set -eu
cd "$(dirname "$0")"
mkdir -p _build
erlc -o _build -Werror ./*.erl
erlc -o _build -pa _build -Werror test/*.erl
erl -noshell -pa _build -eval '
    Modules = [hird_types, hird_tool_dispatch, hird_handlers, hird_audit,
               hird_replay, hird_sup_util, hird_stand, hird_clock],
    case eunit:test(Modules, []) of
        ok -> halt(0);
        _ -> halt(1)
    end.'

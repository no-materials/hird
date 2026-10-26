#!/usr/bin/env sh
# Compiles the runtime and its benchmarks, then runs one scenario, or all
# of them: `bench.sh <scenario> [--quick]`, no arguments for the list.
# Requires Erlang/OTP on PATH; run from anywhere.
set -eu
cd "$(dirname "$0")"
mkdir -p _build
erlc -o _build -Werror ../*.erl ./*.erl
# The report leaves the emulator on stderr: point that at this script's
# stdout, and the emulator's own stdout (the stdout audit sink, logger
# reports) at /dev/null.
exec erl -noshell -pa _build \
    -eval 'hird_bench:main(init:get_plain_arguments())' -extra "$@" \
    2>&1 >/dev/null

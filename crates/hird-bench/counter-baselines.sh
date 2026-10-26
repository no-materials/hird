#!/usr/bin/env bash
# Copyright 2026 the Hird Authors
# SPDX-License-Identifier: Apache-2.0 OR MIT
#
# The work-counter gate, run against the demo programs and hird-bench
# corpora.
#
#   crates/hird-bench/counter-baselines.sh check    # CI: fail on any drift
#   crates/hird-bench/counter-baselines.sh update   # regenerate the baselines
#
# Each program's baseline is `crates/hird-bench/counter-baselines/<name>.json`,
# the counters `hird check --timings=json` reports. Counters are
# deterministic, so a change that moves one must update its baseline in the
# same reviewable diff. Canaries check a shape at two sizes and fail when a
# counter grows faster than the size. Wall time goes to
# `$GITHUB_STEP_SUMMARY` when that is set, and never gates.
#
# Needs jq. `HIRD` and `HIRD_BENCH` override how the two binaries are run.
set -euo pipefail

cd "$(dirname "$0")/../.."
hird=${HIRD:-cargo run --quiet --package hird-cli --}
bench=${HIRD_BENCH:-cargo run --quiet --package hird-bench --}
baselines=crates/hird-bench/counter-baselines
out=_build/counter-baselines

demos=(
  "agent_planner demo/agent_planner.hird"
  "counter_demo  demo/counter_demo.hird"
  "heartbeat     demo/heartbeat.hird"
  "agent_fleet   demo/agent_fleet"
)

# A hird-bench shape and size, at seed 0, named `<shape>-<size>`.
corpora=(
  "functions      500"
  "modules-prev   20"
  "modules-all    20"
  "records        40"
  "tools          50"
  "tools          100"
  "tuple-match    8"
  "tuple-match    16"
  "decision-table 3"
  "decision-table 6"
  "deep           120"
  "actors         50"
)

# A shape, a small and a large size, then the counters that must grow at
# most 1.25 times as fast as the size: doubling the size may take one to
# 2.5x, where quadratic growth reaches 4x.
canaries=(
  "tuple-match    8  16  exhaustiveness_rows exhaustiveness_witnesses"
  "decision-table 3  6   exhaustiveness_rows exhaustiveness_witnesses"
  "tools          50 100 effect_row_merges"
)

# Canaries that are expected to fail; one that passes fails the gate until
# it comes off this list. `tools`: merging each call's row into the body's
# revisits every effect accumulated so far, so effect_row_merges is
# quadratic in the tools one body calls.
known_cliffs=(tools)

mode=${1:-check}
case "$mode" in
  check | update) ;;
  *)
    echo "usage: $0 [check|update]" >&2
    exit 2
    ;;
esac

mkdir -p "$out" "$baselines"
programs=("${demos[@]}")
for entry in "${corpora[@]}"; do
  read -r shape size <<<"$entry"
  src=$($bench "$shape" --size "$size" --corpus-only --out "$out")
  programs+=("$shape-$size $src")
done

status=0
drift=0
times="| program | load | parse | check |"$'\n'"|---|--:|--:|--:|"
for entry in "${programs[@]}"; do
  read -r name input <<<"$entry"
  if ! $hird check --timings=json "$input" >/dev/null 2>"$out/$name.stderr"; then
    cat "$out/$name.stderr" >&2
    exit 1
  fi
  report=$(tail -n 1 "$out/$name.stderr")
  jq -S .counters <<<"$report" >"$out/$name.json"
  times+=$'\n'$(jq -r --arg name "$name" '
    def ms: (. / 100 | round) as $t | "\($t / 10 | floor).\($t % 10) ms";
    (.phases | map({key: .name, value: .wall_us}) | from_entries) as $p
    | "| \($name) | \($p.load | ms) | \($p.parse | ms) | \($p.check | ms) |"' <<<"$report")
  case "$mode" in
    check)
      echo "== $input against $baselines/$name.json"
      diff -u "$baselines/$name.json" "$out/$name.json" || drift=1
      ;;
    update)
      cp "$out/$name.json" "$baselines/$name.json"
      echo "wrote $baselines/$name.json"
      ;;
  esac
done

for entry in "${canaries[@]}"; do
  read -r shape small large counters <<<"$entry"
  grown=
  for counter in $counters; do
    a=$(jq ".$counter" "$out/$shape-$small.json")
    b=$(jq ".$counter" "$out/$shape-$large.json")
    if ((4 * b * small > 5 * a * large)); then
      grown+="${grown:+, }$counter $a → $b"
    fi
  done
  known=0
  for cliff in "${known_cliffs[@]}"; do
    if [[ $cliff == "$shape" ]]; then
      known=1
    fi
  done
  label="canary $shape $small → $large"
  if [[ -z $grown ]] && ((known)); then
    echo "== $label: grows linearly; take it off known_cliffs"
    status=1
  elif [[ -z $grown ]]; then
    echo "== $label: grows linearly"
  elif ((known)); then
    echo "== $label: known cliff: $grown"
  else
    echo "== $label: grows faster than linear: $grown"
    status=1
  fi
done

if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then
  printf '### Wall time (not gated)\n\n%s\n' "$times" >>"$GITHUB_STEP_SUMMARY"
fi
if ((drift)); then
  echo "counters drifted; if that is intended, run $0 update and commit the baselines" >&2
  status=1
fi
exit "$status"

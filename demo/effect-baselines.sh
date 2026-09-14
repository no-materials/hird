#!/usr/bin/env bash
# Copyright 2026 the Hird Authors
# SPDX-License-Identifier: Apache-2.0 OR MIT
#
# The approved-baseline workflow, run against the demo programs.
#
#   demo/effect-baselines.sh check    # CI: fail when any graph drifted
#   demo/effect-baselines.sh update   # regenerate the committed baselines
#
# Each program's baseline is `demo/effect-baselines/<name>.json`. A change
# that moves a graph must update its baseline in the same reviewable diff.
set -euo pipefail

cd "$(dirname "$0")/.."
hird=${HIRD:-cargo run --quiet --package hird-cli --}

programs=(
  "agent_planner demo/agent_planner.hird"
  "counter_demo  demo/counter_demo.hird"
  "heartbeat     demo/heartbeat.hird"
  "agent_fleet   demo/agent_fleet"
)

status=0
for entry in "${programs[@]}"; do
  read -r name input <<<"$entry"
  baseline="demo/effect-baselines/$name.json"
  case "${1:-check}" in
    check)
      echo "== $input against $baseline"
      $hird effect-diff --exact "$baseline" "$input" || status=1
      ;;
    update)
      $hird emit-effect-graph --json "$input" >"$baseline"
      echo "wrote $baseline"
      ;;
    *)
      echo "usage: $0 [check|update]" >&2
      exit 2
      ;;
  esac
done
exit "$status"

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `--timings`: the JSON report's counters are snapshot-tested on the demos
//! (wall times vary run to run, so phases are checked by name only), and
//! the text form and its `--help` documentation are checked for shape.
//! Sources are named by relative paths, because generated Erlang embeds the
//! path and `erl_bytes` counts it. BEAM-dependent tests are skipped (with a
//! note) when `erlc` is not on the `PATH`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

/// The demo programs, relative to this crate (the tests' working directory).
const DEMOS: [(&str, &str); 4] = [
    ("agent_planner", "../../demo/agent_planner.hird"),
    ("counter_demo", "../../demo/counter_demo.hird"),
    ("heartbeat", "../../demo/heartbeat.hird"),
    ("agent_fleet", "../../demo/agent_fleet"),
];

/// Every counter the report carries, in report order.
const COUNTERS: [&str; 13] = [
    "modules",
    "tokens",
    "cst_nodes",
    "typed_nodes",
    "unify_calls",
    "subst_slots",
    "exhaustiveness_rows",
    "exhaustiveness_witnesses",
    "effect_row_merges",
    "erl_bytes",
    "modules_compiled",
    "modules_reused",
    "emulator_boots",
];

/// Runs the `hird` binary with `args`, panicking if it cannot be spawned.
fn hird(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hird"))
        .args(args)
        .output()
        .expect("spawn the hird binary")
}

/// A fresh scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clear scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Whether `erlc` can be spawned (BEAM-dependent tests skip otherwise).
/// Setting `HIRD_REQUIRE_BEAM` refuses the skip: where Erlang is meant to
/// be installed, a missing toolchain is a failure, not a quiet pass.
fn erlang_available() -> bool {
    if Command::new("erlc").arg("-version").output().is_ok() {
        return true;
    }
    assert!(
        std::env::var_os("HIRD_REQUIRE_BEAM").is_none(),
        "HIRD_REQUIRE_BEAM is set but erlc is not on PATH"
    );
    false
}

/// The captured stderr as UTF-8.
fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The `--timings=json` report of a successful run: its last stderr line.
fn report(output: &Output) -> Value {
    let err = stderr(output);
    assert!(output.status.success(), "stderr: {err}");
    let line = err.lines().last().expect("a timings report on stderr");
    serde_json::from_str(line).unwrap_or_else(|e| panic!("not a timings report ({e}): {err}"))
}

/// The report's phase names, in run order.
fn phases(report: &Value) -> Vec<&str> {
    report["phases"]
        .as_array()
        .expect("phases is an array")
        .iter()
        .map(|phase| {
            assert!(phase["wall_us"].is_u64(), "wall_us is a count: {phase}");
            phase["name"].as_str().expect("a phase name")
        })
        .collect()
}

/// The report's counters, pretty-printed for a snapshot.
fn counters(report: &Value) -> String {
    assert_eq!(report["schema_version"], 1, "schema version: {report}");
    serde_json::to_string_pretty(&report["counters"]).expect("render counters")
}

#[test]
fn check_counters_on_the_demos() {
    for (name, path) in DEMOS {
        let report = report(&hird(&["check", "--timings=json", path]));
        assert_eq!(phases(&report), ["load", "parse", "check"], "{name}");
        insta::assert_snapshot!(format!("check_{name}"), counters(&report));
    }
}

#[test]
fn build_and_run_counters_on_the_counter_demo() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("timings_build_run");
    let (_, path) = DEMOS[1];
    let out = dir.join("out").display().to_string();

    let build = report(&hird(&["build", "--timings=json", path, "-o", &out]));
    assert_eq!(
        phases(&build),
        ["load", "parse", "check", "lower", "emit", "write", "erlc"]
    );
    insta::assert_snapshot!("build_counter_demo", counters(&build));

    let run = report(&hird(&["run", "--timings=json", path, "-o", &out]));
    assert_eq!(
        phases(&run),
        [
            "load", "parse", "check", "lower", "emit", "write", "erlc", "run"
        ]
    );
    insta::assert_snapshot!("run_counter_demo", counters(&run));
}

#[test]
fn text_timings_list_every_phase_and_counter() {
    let (_, path) = DEMOS[0];
    // A bare `--timings` takes no value, so the path after it is the input.
    let output = hird(&["check", "--timings", path]);
    let err = stderr(&output);
    assert!(output.status.success(), "stderr: {err}");
    let report = err.split_once("timings:\n").expect("a text report").1;
    for name in ["load", "parse", "check", "total", "counters:"]
        .into_iter()
        .chain(COUNTERS)
    {
        assert!(
            report
                .lines()
                .any(|line| line.trim_start().starts_with(name)),
            "`{name}` missing from:\n{report}"
        );
    }
}

#[test]
fn timings_are_documented_in_help() {
    for sub in ["check", "build", "run"] {
        let output = hird(&[sub, "--help"]);
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(help.contains("--timings[=<FORMAT>]"), "{sub}: {help}");
        for name in COUNTERS {
            assert!(help.contains(name), "{sub} --help omits `{name}`: {help}");
        }
    }
}

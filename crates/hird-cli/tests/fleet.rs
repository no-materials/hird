// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! End-to-end coverage of the standing agent-fleet demo
//! (`demo/agent_fleet/`): a two-module source tree checked, built, and run
//! standing on BEAM, with the audit stream asserting periodic rounds, the
//! deliberate round-3 crash, and the `rest_for_one` recovery. BEAM-dependent
//! tests are skipped (with a note) when `erlc` is not on the `PATH`.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// The demo source directory checked into the repository.
fn fleet_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/agent_fleet")
}

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

/// The captured stdout as UTF-8.
fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn fleet_check_passes() {
    let output = hird(&["check", fleet_path().to_str().expect("utf-8 path")]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
}

/// The build emits one base module per source file — the cross-module
/// emit path, exercised end to end — plus a behaviour module per actor
/// and the supervisor.
#[test]
fn fleet_build_emits_modules_for_both_source_files() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("fleet_build");
    let out_dir = dir.join("out");
    let output = hird(&[
        "build",
        fleet_path().to_str().expect("utf-8 path"),
        "-o",
        out_dir.to_str().expect("utf-8 path"),
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    for name in [
        "hird_agent_fleet.erl",
        "hird_agent_fleet.beam",
        "hird_errands.erl",
        "hird_errands.beam",
        "hird_planner.beam",
        "hird_executor.beam",
        "hird_auditor.beam",
        "hird_fleet_sup.beam",
        "hird_boot.beam",
    ] {
        assert!(out_dir.join(name).exists(), "missing {name}");
    }
    // The executor's forged orders resolve to the defining module: the
    // planner calls Errands.forge across the module boundary.
    let planner = fs::read_to_string(out_dir.join("hird_planner.erl")).expect("read the planner");
    assert!(
        planner.contains("hird_errands:forge("),
        "planner module: {planner}"
    );
}

/// The effect graph names the whole tree across both modules: the live
/// org chart of the standing fleet.
#[test]
fn fleet_effect_graph_names_the_tree() {
    let output = hird(&[
        "emit-effect-graph",
        fleet_path().to_str().expect("utf-8 path"),
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let out = stdout(&output);
    for line in [
        "module AgentFleet",
        "module Errands",
        "actor Planner",
        "actor Executor",
        "actor Auditor",
        "supervisor FleetSup",
        "strategy rest_for_one (intensity 5, period 60)",
        "child planner: Planner (permanent)",
        "child executor: Executor (permanent)",
        "child auditor: Auditor (permanent)",
    ] {
        assert!(out.contains(line), "missing `{line}`: {out}");
    }
}

/// The flagship run: the fleet stands, rounds beat periodically, round 3
/// crashes the executor on purpose, `rest_for_one` restarts the executor
/// and the auditor (never the planner), and the rounds keep coming.
/// Unix only, like every standing test: it sends SIGINT.
#[test]
fn fleet_stands_crashes_and_recovers() {
    if !cfg!(unix) {
        eprintln!("skipping: the test sends SIGINT, which is Unix-only");
        return;
    }
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("fleet_run");
    let audit = dir.join("audit.jsonl");
    let mut child = Command::new(env!("CARGO_BIN_EXE_hird"))
        .args([
            "run",
            fleet_path().to_str().expect("utf-8 path"),
            "-o",
            dir.join("out").to_str().expect("utf-8 path"),
            "--audit-file",
            audit.to_str().expect("utf-8 path"),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the hird binary");

    // Round 4 chronicled: the fleet survived the round-3 crash and kept
    // its round counter — the planner was never restarted.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let log = fs::read_to_string(&audit).unwrap_or_default();
        if log
            .lines()
            .any(|l| l.contains("\"tool\":\"Chronicle\"") && l.contains("\"round\":4"))
        {
            break;
        }
        if let Some(status) = child.try_wait().expect("poll the hird binary") {
            let mut err = String::new();
            if let Some(stderr) = child.stderr.as_mut() {
                let _ = stderr.read_to_string(&mut err);
            }
            panic!("the program exited early with {status}; stderr: {err}");
        }
        assert!(
            Instant::now() < deadline,
            "round 4 was never chronicled; audit log: {log}"
        );
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        child.try_wait().expect("poll the hird binary").is_none(),
        "the program halted instead of standing"
    );

    let interrupt = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT");
    assert!(interrupt.success(), "SIGINT was not delivered");
    let output = child.wait_with_output().expect("wait for the hird binary");
    assert!(
        output.status.success(),
        "status: {:?}, stderr: {}",
        output.status,
        stderr(&output)
    );

    let log = fs::read_to_string(&audit).expect("read the audit log");
    let count = |needle: &str| log.matches(needle).count();

    // Two audit beats per surviving round — the errand run, then its
    // chronicle — with the duty forged across the module boundary.
    for round in [1, 2, 4] {
        for tool in ["RunErrand", "Chronicle"] {
            assert!(
                log.lines()
                    .any(|l| l.contains(&format!("\"tool\":\"{tool}\""))
                        && l.contains(&format!("\"round\":{round}"))),
                "missing {tool} beat for round {round}: {log}"
            );
        }
    }
    assert!(
        log.contains("\"errand\":\"scout the border\""),
        "audit log: {log}"
    );

    // Round 3 has no beat at all: the crash consumed the order before
    // the errand ran, and the restarted executor never saw it again.
    assert_eq!(count("\"round\":3"), 0, "audit log: {log}");

    // rest_for_one restarted the crashed executor and its downstream
    // auditor exactly once each; the planner kept its post throughout.
    assert_eq!(count("planner takes its post"), 1, "audit log: {log}");
    assert_eq!(count("executor takes its post"), 2, "audit log: {log}");
    assert_eq!(count("auditor takes its post"), 2, "audit log: {log}");
}

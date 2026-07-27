// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! End-to-end coverage of the `hird` binary: every subcommand, plus the
//! entry-point and Erlang-detection error paths. BEAM-dependent tests are
//! skipped (with a note) when `erlc` is not on the `PATH`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A self-contained program with a tool, a mock handler, and `fn main`.
const PING: &str = "effect Tool<t>\n\
     tool Ping : { msg: String } -> String\n\
     fn fake_ping(args: { msg: String }) -> String ! {} = \"pong\"\n\
     fn main() ! {} =\n\
       handle { Tool<Ping> -> fake_ping } in\n\
         match ping({ msg: \"hi\" }) { _ -> () }";

/// An actor and supervisor module with a tool, for the effect graph.
const PLANNER: &str = "effect Tool<t>\n\
     type Path = Path(String)\n\
     type RepoState = RepoState(String)\n\
     type St = St(Int)\n\
     tool ReadRepo : { path: Path } -> RepoState\n\
     fn read(p: Path, st: St) -> St ! {Tool<ReadRepo>} =\n\
       match read_repo({ path: p }) { _ -> st }\n\
     fn default_config() -> St = St(0)\n\
     actor Planner {\n\
       state: St,\n\
       message: PlannerMsg = | PlanRepo(Path),\n\
       init: fn(c: St) -> St ! {} = c,\n\
       handle PlanRepo(p), st -> St ! {Tool<ReadRepo>} = read(p, st),\n\
     } ! {Tool<ReadRepo>}\n\
     supervisor PlannerSup {\n\
       strategy: one_for_one,\n\
       intensity: 5,\n\
       period: 60,\n\
       children: [\n\
         { id: planner, actor: Planner, start_args: default_config(), restart: permanent },\n\
       ]\n\
     }";

/// A program whose `main` leaks a tool effect (no `handle` block).
const LEAKY: &str = "effect Tool<t>\n\
     tool Ping : { msg: String } -> String\n\
     fn main() ! {Tool<Ping>} = match ping({ msg: \"hi\" }) { _ -> () }";

/// A module with no `fn main`.
const LIB: &str = "fn helper(x: Int) -> Int = x + 1";

/// A file with a type error.
const BAD: &str = "fn add(x: Int) -> Int = x + \"no\"";

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

/// Writes `source` as `<dir>/<name>`, returning the path as a `String`.
fn write(dir: &Path, name: &str, source: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, source).expect("write test source");
    path.display().to_string()
}

/// Whether `erlc` can be spawned (BEAM-dependent tests skip otherwise).
fn erlang_available() -> bool {
    Command::new("erlc").arg("-version").output().is_ok()
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
fn check_passes_a_valid_program() {
    let dir = scratch("check_ok");
    let file = write(&dir, "main.hird", PING);
    let output = hird(&["check", &file]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("checked 1 module(s)"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn check_reports_type_errors_with_spans() {
    let dir = scratch("check_bad");
    let file = write(&dir, "bad.hird", BAD);
    let output = hird(&["check", &file]);
    assert!(!output.status.success(), "type errors must fail the check");
    let err = stderr(&output);
    assert!(err.contains("type mismatch"), "stderr: {err}");
    assert!(err.contains("bad.hird"), "report names the file: {err}");
}

#[test]
fn check_walks_a_directory_of_modules() {
    let dir = scratch("check_dir");
    write(&dir, "main.hird", PING);
    write(&dir, "util.hird", LIB);
    let output = hird(&["check", &dir.display().to_string()]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(
        stderr(&output).contains("checked 2 module(s)"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn build_produces_erl_runtime_boot_and_beam() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("build_ok");
    let file = write(&dir, "main.hird", PING);
    let out_dir = dir.join("out");
    let output = hird(&["build", &file, "-o", &out_dir.display().to_string()]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    for name in [
        "hird_main.erl",
        "hird_main.beam",
        "hird_boot.erl",
        "hird_boot.beam",
        "hird_audit.erl",
        "hird_tool_dispatch.beam",
    ] {
        assert!(out_dir.join(name).exists(), "missing {name}");
    }
}

#[test]
fn build_rejects_main_with_unhandled_tool_effects() {
    let dir = scratch("build_leaky");
    let file = write(&dir, "main.hird", LEAKY);
    let out_dir = dir.join("out");
    let output = hird(&["build", &file, "-o", &out_dir.display().to_string()]);
    assert!(!output.status.success(), "a leaky main must fail the build");
    assert!(
        stderr(&output).contains("unhandled tool effects"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn run_executes_on_beam_and_audits_tool_calls() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("run_ok");
    let file = write(&dir, "main.hird", PING);
    let out_dir = dir.join("out");
    let output = hird(&["run", &file, "-o", &out_dir.display().to_string()]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("\"tool\":\"Ping\""), "stdout: {out}");
    assert!(
        out.contains("\"result\":{\"ok\":\"pong\"}"),
        "stdout: {out}"
    );
    assert!(out.contains("\"caller\":\"Main.main\""), "stdout: {out}");
}

#[test]
fn run_requires_a_main_function() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("run_no_main");
    let file = write(&dir, "lib.hird", LIB);
    let out_dir = dir.join("out");
    let output = hird(&["run", &file, "-o", &out_dir.display().to_string()]);
    assert!(!output.status.success(), "run without main must fail");
    assert!(
        stderr(&output).contains("no `fn main` found"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn run_reserves_main_arguments() {
    let dir = scratch("run_args");
    let file = write(&dir, "main.hird", PING);
    let output = hird(&["run", &file, "--", "foo"]);
    assert!(!output.status.success(), "trailing args must be rejected");
    assert!(
        stderr(&output).contains("reserved"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn emit_ast_prints_canonical_source_and_json() {
    let dir = scratch("emit_ast");
    let file = write(&dir, "main.hird", PING);

    let text = hird(&["emit-ast", &file]);
    assert!(text.status.success(), "stderr: {}", stderr(&text));
    assert!(
        stdout(&text).contains("fn fake_ping"),
        "stdout: {}",
        stdout(&text)
    );

    let json = hird(&["emit-ast", &file, "--json"]);
    assert!(json.status.success(), "stderr: {}", stderr(&json));
    let out = stdout(&json);
    assert!(out.trim_start().starts_with('{'), "stdout: {out}");
    assert!(out.contains("\"kind\": \"Tool\""), "stdout: {out}");
}

#[test]
fn emit_effect_graph_prints_text_and_json() {
    let dir = scratch("emit_graph");
    let file = write(&dir, "planner.hird", PLANNER);

    let text = hird(&["emit-effect-graph", &file]);
    assert!(text.status.success(), "stderr: {}", stderr(&text));
    let out = stdout(&text);
    assert!(out.contains("actor Planner"), "stdout: {out}");
    assert!(out.contains("supervisor PlannerSup"), "stdout: {out}");
    assert!(out.contains("tool ReadRepo"), "stdout: {out}");

    let json = hird(&["emit-effect-graph", &file, "--json"]);
    assert!(json.status.success(), "stderr: {}", stderr(&json));
    let out = stdout(&json);
    assert!(out.contains("\"schema_version\": 1"), "stdout: {out}");
    assert!(out.contains("\"one_for_one\""), "stdout: {out}");
    assert!(out.contains("\"Tool\""), "stdout: {out}");
}

#[test]
fn missing_erlang_produces_install_advice() {
    let dir = scratch("no_erlang");
    let file = write(&dir, "main.hird", PING);
    let out_dir = dir.join("out");
    let output = Command::new(env!("CARGO_BIN_EXE_hird"))
        .args(["build", &file, "-o", &out_dir.display().to_string()])
        .env("PATH", "")
        .output()
        .expect("spawn the hird binary");
    assert!(!output.status.success(), "missing erlc must fail the build");
    assert!(
        stderr(&output).contains("Erlang/OTP not found"),
        "stderr: {}",
        stderr(&output)
    );
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! End-to-end coverage of the `hird` binary: every subcommand, plus the
//! entry-point and Erlang-detection error paths. BEAM-dependent tests are
//! skipped (with a note) when `erlc` is not on the `PATH`.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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

/// A standing program: a supervised actor that logs each note it is sent,
/// and a `main` that supervises it, sends one note, then stands.
const STANDING: &str = "effect Tool<t>\n\
     effect Send<t>\n\
     type St = St(Int)\n\
     tool Log : { message: String } -> ()\n\
     fn fake_log(args: { message: String }) -> () = ()\n\
     fn config() -> St = St(0)\n\
     actor Keeper {\n\
       state: St,\n\
       message: KeeperMsg = | Note(String),\n\
       init: fn(c: St) -> St ! {} = c,\n\
       handle Note(m), st -> St ! {Tool<Log>} = match log({ message: m }) { _ -> st },\n\
     } ! {Tool<Log>}\n\
     supervisor KeeperSup {\n\
       strategy: one_for_one,\n\
       intensity: 5,\n\
       period: 60,\n\
       children: [\n\
         { id: keeper, actor: Keeper, start_args: config(), restart: permanent },\n\
       ]\n\
     }\n\
     fn main() ! {Install, Supervise, Send<KeeperMsg>, Stand} =\n\
       install { Tool<Log> -> fake_log } in\n\
       let u = supervise(KeeperSup) in\n\
       let s = send(child(KeeperSup, keeper), Note(\"standing\")) in\n\
       stand()";

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

/// Blanks every `"timestamp":"…"` value, which differs across runs.
fn strip_timestamps(log: &str) -> String {
    let key = "\"timestamp\":\"";
    let mut out = String::new();
    let mut rest = log;
    while let Some(i) = rest.find(key) {
        let start = i + key.len();
        out.push_str(&rest[..start]);
        let end = start + rest[start..].find('"').expect("unterminated timestamp");
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
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
fn run_audit_file_redirects_the_audit_stream() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("run_audit_file");
    let file = write(&dir, "main.hird", PING);

    let plain_out = dir.join("out_plain");
    let plain = hird(&["run", &file, "-o", &plain_out.display().to_string()]);
    assert!(plain.status.success(), "stderr: {}", stderr(&plain));

    let audit = dir.join("audit.jsonl");
    let flagged_out = dir.join("out_flagged");
    let flagged = hird(&[
        "run",
        &file,
        "-o",
        &flagged_out.display().to_string(),
        "--audit-file",
        &audit.display().to_string(),
    ]);
    assert!(flagged.status.success(), "stderr: {}", stderr(&flagged));
    assert_eq!(
        stdout(&flagged),
        "",
        "no audit lines may reach stdout with --audit-file"
    );
    let logged = fs::read_to_string(&audit).expect("read the audit file");
    assert_eq!(
        strip_timestamps(&logged),
        strip_timestamps(&stdout(&plain)),
        "the audit file must carry the stream an unflagged run prints"
    );
}

#[test]
fn run_replay_reproduces_a_recorded_run() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("run_replay");
    let file = write(&dir, "main.hird", PING);

    let recorded = dir.join("recorded.jsonl");
    let record = hird(&[
        "run",
        &file,
        "-o",
        &dir.join("out_record").display().to_string(),
        "--audit-file",
        &recorded.display().to_string(),
    ]);
    assert!(record.status.success(), "stderr: {}", stderr(&record));

    let replayed = dir.join("replayed.jsonl");
    let replay = hird(&[
        "run",
        &file,
        "-o",
        &dir.join("out_replay").display().to_string(),
        "--replay",
        &recorded.display().to_string(),
        "--audit-file",
        &replayed.display().to_string(),
    ]);
    assert!(replay.status.success(), "stderr: {}", stderr(&replay));
    let recorded_log = fs::read_to_string(&recorded).expect("read the recorded log");
    let replayed_log = fs::read_to_string(&replayed).expect("read the replayed log");
    assert_eq!(
        strip_timestamps(&replayed_log),
        strip_timestamps(&recorded_log),
        "a replayed run must audit the recorded tool/args/result sequence"
    );
}

#[test]
fn run_replay_rejects_a_tampered_log() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("run_replay_tampered");
    let file = write(&dir, "main.hird", PING);

    let recorded = dir.join("recorded.jsonl");
    let record = hird(&[
        "run",
        &file,
        "-o",
        &dir.join("out_record").display().to_string(),
        "--audit-file",
        &recorded.display().to_string(),
    ]);
    assert!(record.status.success(), "stderr: {}", stderr(&record));

    let log = fs::read_to_string(&recorded).expect("read the recorded log");
    let tampered = log.replace("\"msg\":\"hi\"", "\"msg\":\"ho\"");
    assert_ne!(tampered, log, "the tamper must change the recorded args");
    fs::write(&recorded, tampered).expect("write the tampered log");

    let replay = hird(&[
        "run",
        &file,
        "-o",
        &dir.join("out_replay").display().to_string(),
        "--replay",
        &recorded.display().to_string(),
    ]);
    assert!(!replay.status.success(), "a tampered log must fail the run");
    let err = stderr(&replay);
    assert!(err.contains("replay_divergence"), "stderr: {err}");
    assert!(err.contains("args_mismatch"), "stderr: {err}");
    assert!(err.contains("position => 0"), "stderr: {err}");
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

// ── standing programs ───────────────────────────────────────────

/// Polls `path` until it contains `needle`; panics after `timeout`, or as
/// soon as `child` (the program writing the file) exits.
fn wait_for_content(child: &mut Child, path: &Path, needle: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if fs::read_to_string(path)
            .unwrap_or_default()
            .contains(needle)
        {
            return;
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
            "`{}` never contained `{needle}`",
            path.display()
        );
        thread::sleep(Duration::from_millis(50));
    }
}

/// A `main` that stands keeps its supervised actor serving after its own
/// setup is done, and Ctrl-C to `hird run` shuts the tree down cleanly:
/// exit status 0 with the audit stream synced.
#[test]
fn run_stands_until_interrupted_then_exits_cleanly() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("run_stands");
    let file = write(&dir, "standing.hird", STANDING);
    let audit = dir.join("audit.jsonl");
    let mut child = Command::new(env!("CARGO_BIN_EXE_hird"))
        .args([
            "run",
            &file,
            "-o",
            dir.join("out").to_str().expect("utf-8 path"),
            "--audit-file",
            audit.to_str().expect("utf-8 path"),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the hird binary");

    // The actor served the note (main has moved on to standing by the time
    // an unrelated process observes the write), and the program is still up.
    wait_for_content(
        &mut child,
        &audit,
        "\"tool\":\"Log\"",
        Duration::from_secs(60),
    );
    thread::sleep(Duration::from_millis(200));
    assert!(
        child.try_wait().expect("poll the hird binary").is_none(),
        "the program halted instead of standing"
    );

    let interrupt = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT");
    assert!(interrupt.success());
    let output = child.wait_with_output().expect("wait for the hird binary");
    assert!(
        output.status.success(),
        "status: {:?}, stderr: {}",
        output.status,
        stderr(&output)
    );

    // The audit stream was synced before the halt: one complete record.
    let log = fs::read_to_string(&audit).expect("read the audit log");
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 1, "audit log: {log}");
    assert!(
        lines[0].contains("\"args\":{\"message\":\"standing\"}"),
        "audit log: {log}"
    );
    assert!(lines[0].ends_with('}'), "audit log: {log}");
}

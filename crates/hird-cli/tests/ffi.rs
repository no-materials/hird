// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Erlang-bound externs end to end: a tool implemented by a hand-written
//! Erlang module next to the `.hird` source runs under `hird run`, every
//! call is on the audit stream, and a replay serves the results without
//! calling the Erlang code. The Erlang side appends each call it receives
//! to `calls.log`, so a test sees exactly which runs reached it. A replay
//! whose cursor is killed mid-run fails without ever reaching the Erlang
//! code. BEAM-dependent tests are skipped (with a note) when `erlc` is not
//! on the `PATH`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// A program reading one note through an Erlang-implemented tool and
/// logging it through a Hirð mock.
const NOTES: &str = "module Notes\n\
     \n\
     extern module \"ffi/notes.erl\"\n\
     \n\
     type NoteError = Missing(String)\n\
     tool ReadNote : { name: String } → String ! {Exn<NoteError>}\n\
     tool Log : { message: String } → ()\n\
     \n\
     extern fn erl_read_note(args: { name: String }) → String ! {Exn<NoteError>}\n\
       = \"notes\":\"read\"\n\
     fn quiet_log(args: { message: String }) → () = ()\n\
     \n\
     fn main() → () ! {Exn<NoteError>} =\n\
       handle { Tool<ReadNote> → erl_read_note, Tool<Log> → quiet_log } in\n\
         log({ message: read_note({ name: \"todo\" }) })\n";

/// The Erlang tool: reads `<name>.txt` from `NOTES_DIR`, logging the call
/// first, and raises `Missing(name)` when there is no such note.
const NOTES_ERL: &str = "-module(notes).\n\
     -export([read/1]).\n\
     \n\
     read(#{name := Name}) ->\n\
     \x20   Dir = os:getenv(\"NOTES_DIR\"),\n\
     \x20   ok = file:write_file(filename:join(Dir, \"calls.log\"), [Name, $\\n], [append]),\n\
     \x20   case file:read_file(filename:join(Dir, <<Name/binary, \".txt\">>)) of\n\
     \x20       {ok, Text} -> Text;\n\
     \x20       {error, enoent} -> throw({hird_exn, {missing, Name}})\n\
     \x20   end.\n";

/// A fresh scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clear scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Writes the notes program and its Erlang tool (`erl`) under `dir`,
/// returning the program's path.
fn write_program(dir: &Path, erl: &str) -> PathBuf {
    fs::create_dir_all(dir.join("ffi")).expect("create ffi dir");
    fs::write(dir.join("ffi/notes.erl"), erl).expect("write the Erlang tool");
    let source = dir.join("notes.hird");
    fs::write(&source, NOTES).expect("write the program");
    source
}

/// Whether `erlc` is on `PATH` (BEAM-dependent tests skip otherwise),
/// looked up rather than spawned: any `erlc` run boots an emulator.
/// Setting `HIRD_REQUIRE_BEAM` refuses the skip: where Erlang is meant to
/// be installed, a missing toolchain is a failure, not a quiet pass.
fn erlang_available() -> bool {
    let erlc = format!("erlc{}", std::env::consts::EXE_SUFFIX);
    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(&erlc).is_file()));
    if on_path {
        return true;
    }
    assert!(
        std::env::var_os("HIRD_REQUIRE_BEAM").is_none(),
        "HIRD_REQUIRE_BEAM is set but erlc is not on PATH"
    );
    false
}

/// Runs `hird <args>` with the notes directory and build cache under `dir`.
fn hird(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hird"))
        .env("HIRD_CACHE_DIR", dir.join("cache"))
        .env("NOTES_DIR", dir)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("spawn the hird binary")
}

/// The calls the Erlang tool received, one name per line.
fn calls(dir: &Path) -> String {
    fs::read_to_string(dir.join("calls.log")).unwrap_or_default()
}

/// The lines of `output`'s stdout naming `tool`, envelope fields after the
/// timestamp dropped, so a live and a replayed line compare equal.
fn records(output: &Output, tool: &str) -> Vec<String> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains(&format!("\"tool\":\"{tool}\"")))
        .map(|line| {
            line.split(",\"timestamp\"")
                .next()
                .unwrap_or(line)
                .to_owned()
        })
        .collect()
}

#[test]
fn a_real_tool_is_recorded_and_replays_without_running() {
    if !erlang_available() {
        eprintln!("skipping: erlc not on PATH");
        return;
    }
    let dir = scratch("ffi_record_replay");
    write_program(&dir, NOTES_ERL);
    fs::write(dir.join("todo.txt"), "buy milk").expect("write the note");

    let live = hird(&dir, &["run", "notes.hird", "-o", "out"]);
    assert!(
        live.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&live.stderr)
    );
    assert_eq!(calls(&dir), "todo\n");
    let read = records(&live, "ReadNote");
    assert_eq!(
        read,
        [
            "{\"schema_version\":1,\"tool\":\"ReadNote\",\"args\":{\"name\":\"todo\"},\
          \"result\":{\"ok\":\"buy milk\"}"
        ]
    );
    assert_eq!(records(&live, "Log").len(), 1);

    // Record, then take the note away: the replay must not need it.
    let recorded = hird(
        &dir,
        &[
            "run",
            "notes.hird",
            "-o",
            "out",
            "--audit-file",
            "rec.jsonl",
        ],
    );
    assert!(recorded.status.success());
    assert_eq!(calls(&dir), "todo\ntodo\n");
    fs::remove_file(dir.join("todo.txt")).expect("remove the note");
    fs::remove_file(dir.join("calls.log")).expect("reset the call log");

    let replayed = hird(
        &dir,
        &["run", "notes.hird", "-o", "out", "--replay", "rec.jsonl"],
    );
    assert!(
        replayed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&replayed.stderr)
    );
    assert_eq!(calls(&dir), "", "the replay reached the Erlang tool");
    assert_eq!(records(&replayed, "ReadNote"), read);
}

/// An error the Erlang side raises is recorded as the tool's `err` result
/// and replayed as the same error, without the Erlang side running.
#[test]
fn a_raised_error_is_recorded_and_replayed() {
    if !erlang_available() {
        eprintln!("skipping: erlc not on PATH");
        return;
    }
    let dir = scratch("ffi_raised_error");
    write_program(&dir, NOTES_ERL);

    let recorded = hird(
        &dir,
        &[
            "run",
            "notes.hird",
            "-o",
            "out",
            "--audit-file",
            "rec.jsonl",
        ],
    );
    assert!(!recorded.status.success());
    let log = fs::read_to_string(dir.join("rec.jsonl")).expect("the recording");
    assert!(
        log.contains("\"result\":{\"err\":{\"ctor\":\"Missing\",\"args\":[\"todo\"]}}"),
        "recording: {log}"
    );
    assert_eq!(calls(&dir), "todo\n");
    fs::remove_file(dir.join("calls.log")).expect("reset the call log");

    let replayed = hird(
        &dir,
        &["run", "notes.hird", "-o", "out", "--replay", "rec.jsonl"],
    );
    let stderr = String::from_utf8_lossy(&replayed.stderr);
    assert!(!replayed.status.success());
    assert!(
        stderr.contains("{hird_exn,{missing,<<\"todo\">>}}"),
        "stderr: {stderr}"
    );
    assert_eq!(calls(&dir), "");
}

/// A result outside the tool's declared type crashes the caller with an
/// error naming the tool, and is not recorded.
#[test]
fn an_ill_typed_result_crashes_naming_the_tool() {
    if !erlang_available() {
        eprintln!("skipping: erlc not on PATH");
        return;
    }
    let dir = scratch("ffi_ill_typed");
    write_program(
        &dir,
        "-module(notes).\n-export([read/1]).\nread(_) -> {ok, <<\"text\">>}.\n",
    );
    let output = hird(
        &dir,
        &[
            "run",
            "notes.hird",
            "-o",
            "out",
            "--audit-file",
            "rec.jsonl",
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("{handler_result,read_note,")
            && stderr.contains("{unencodable,string,{ok,<<\"text\">>}}}"),
        "stderr: {stderr}"
    );
    let log = fs::read_to_string(dir.join("rec.jsonl")).unwrap_or_default();
    assert!(!log.contains("ReadNote"), "recording: {log}");
}

/// A declared Erlang source that is not there fails the build, naming the
/// file and its declaring program; one that does not compile fails it with
/// the compiler's report.
#[test]
fn a_missing_or_broken_erlang_source_fails_the_build() {
    let dir = scratch("ffi_missing_source");
    fs::write(dir.join("notes.hird"), NOTES).expect("write the program");
    let output = hird(&dir, &["build", "notes.hird", "-o", "out"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("cannot read `ffi/notes.erl`, declared by `notes.hird`"),
        "stderr: {stderr}"
    );

    if !erlang_available() {
        eprintln!("skipping: erlc not on PATH");
        return;
    }
    write_program(&dir, "-module(notes).\n-export([read/1]).\nread(X) -> X\n");
    let output = hird(&dir, &["build", "notes.hird", "-o", "out"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("notes.erl"), "stderr: {stderr}");
}

/// Two modules declaring one Erlang module at two paths fail the build.
#[test]
fn one_erlang_module_has_one_path() {
    let dir = scratch("ffi_two_paths");
    write_program(&dir, NOTES_ERL);
    fs::create_dir_all(dir.join("vendor")).expect("create vendor dir");
    fs::write(dir.join("vendor/notes.erl"), NOTES_ERL).expect("write a second copy");
    fs::write(
        dir.join("helper.hird"),
        "module Helper\n\
         extern module \"vendor/notes.erl\"\n\
         tool Peek : { name: String } → String\n\
         extern fn erl_peek(args: { name: String }) → String = \"notes\":\"read\"\n\
         pub fn glance(name: String) → String =\n\
           handle { Tool<Peek> → erl_peek } in peek({ name: name })\n",
    )
    .expect("write the helper");
    let output = hird(&dir, &["build", ".", "-o", "out"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("Erlang module `notes` is declared at two paths"),
        "stderr: {stderr}"
    );
}

/// An audit sink killed mid-run fails the run with status 1 and the
/// sink's exit reason, rather than letting it finish with records lost.
/// The Erlang tool kills the sink, then holds the call open long enough
/// that only the sink's exit can end the run.
#[test]
fn a_killed_audit_sink_fails_the_run() {
    if !erlang_available() {
        eprintln!("skipping: erlc not on PATH");
        return;
    }
    let dir = scratch("ffi_killed_sink");
    write_program(
        &dir,
        "-module(notes).\n-export([read/1]).\n\
         read(_) ->\n\
         \x20   exit(whereis(hird_audit), kill),\n\
         \x20   timer:sleep(5000),\n\
         \x20   <<\"text\">>.\n",
    );
    let output = hird(
        &dir,
        &[
            "run",
            "notes.hird",
            "-o",
            "out",
            "--audit-file",
            "rec.jsonl",
        ],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        stderr.contains("hird: audit sink exited: killed"),
        "stderr: {stderr}"
    );
}

/// A program making 20000 calls through an Erlang tool that appends each
/// call it receives to `$LIVE_LOG`, when that is set.
const PINGS: &str = "module Pings\n\
     \n\
     extern module \"ffi/pinger.erl\"\n\
     \n\
     tool Ping : { n: Int } → Int\n\
     extern fn erl_ping(args: { n: Int }) → Int = \"pinger\":\"ping\"\n\
     \n\
     fn pings(n: Int) → () ! {Tool<Ping>} =\n\
       if n == 0 then () else match ping({ n: n }) { _ → pings(n - 1) }\n\
     \n\
     fn main() → () =\n\
       handle { Tool<Ping> → erl_ping } in pings(20000)\n";

/// The Erlang side of [`PINGS`].
const PINGER_ERL: &str = "-module(pinger).\n\
     -export([ping/1]).\n\
     \n\
     ping(#{n := N}) ->\n\
     \x20   case os:getenv(\"LIVE_LOG\") of\n\
     \x20       false -> ok;\n\
     \x20       Path -> ok = file:write_file(Path, [integer_to_list(N), $\\n], [append])\n\
     \x20   end,\n\
     \x20   N.\n";

/// Test scaffolding that kills the replay cursor from inside the node,
/// started through `ERL_AFLAGS` as `-s cursor_killer <fun>`: no handler
/// runs under replay, so no tool can. Each function returns at once, so
/// the node boots on.
const CURSOR_KILLER_ERL: &str = "-module(cursor_killer).\n\
     -export([after_reply/0, at_rest/0]).\n\
     \n\
     %% Kills the cursor right after it answers a call, so the program's\n\
     %% next call finds it gone.\n\
     after_reply() ->\n\
     \x20   _ = spawn(fun() ->\n\
     \x20       sys:install(cursor(), {fun kill_after_reply/3, armed})\n\
     \x20   end),\n\
     \x20   ok.\n\
     \n\
     kill_after_reply(armed, Event, _State) when element(1, Event) =:= out ->\n\
     \x20   exit(self(), kill);\n\
     kill_after_reply(armed, _Event, _State) ->\n\
     \x20   armed.\n\
     \n\
     %% Kills the cursor 300 ms after it starts.\n\
     at_rest() ->\n\
     \x20   _ = spawn(fun() ->\n\
     \x20       Cursor = cursor(),\n\
     \x20       timer:sleep(300),\n\
     \x20       exit(Cursor, kill)\n\
     \x20   end),\n\
     \x20   ok.\n\
     \n\
     %% The replay cursor, once it is registered.\n\
     cursor() ->\n\
     \x20   case whereis(hird_replay) of\n\
     \x20       undefined -> timer:sleep(1), cursor();\n\
     \x20       Pid -> Pid\n\
     \x20   end.\n";

/// Compiles [`CURSOR_KILLER_ERL`] into `dir/killer`, returning the
/// `ERL_AFLAGS` that start `function` in a node run from `dir`.
fn cursor_killer(dir: &Path, function: &str) -> String {
    let killer = dir.join("killer");
    fs::create_dir_all(&killer).expect("create killer dir");
    let source = killer.join("cursor_killer.erl");
    fs::write(&source, CURSOR_KILLER_ERL).expect("write the killer");
    let status = Command::new("erlc")
        .arg("-o")
        .arg(&killer)
        .arg(&source)
        .status()
        .expect("spawn erlc");
    assert!(status.success(), "the killer compiles");
    format!("-pa killer -s cursor_killer {function}")
}

/// A replay whose cursor is killed between two calls never runs the
/// program's next call live: the run fails with status 1 naming the
/// cursor, and the Erlang tool is never reached.
#[test]
fn a_killed_replay_cursor_never_runs_a_call_live() {
    if !erlang_available() {
        eprintln!("skipping: erlc not on PATH");
        return;
    }
    let dir = scratch("ffi_killed_cursor");
    fs::create_dir_all(dir.join("ffi")).expect("create ffi dir");
    fs::write(dir.join("ffi/pinger.erl"), PINGER_ERL).expect("write the Erlang tool");
    fs::write(dir.join("pings.hird"), PINGS).expect("write the program");
    let recorded = hird(
        &dir,
        &[
            "run",
            "pings.hird",
            "-o",
            "out",
            "--audit-file",
            "rec.jsonl",
        ],
    );
    assert!(
        recorded.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&recorded.stderr)
    );

    let replayed = Command::new(env!("CARGO_BIN_EXE_hird"))
        .env("HIRD_CACHE_DIR", dir.join("cache"))
        .env("LIVE_LOG", dir.join("live.log"))
        .env("ERL_AFLAGS", cursor_killer(&dir, "after_reply"))
        .current_dir(&dir)
        .args(["run", "pings.hird", "-o", "out", "--replay", "rec.jsonl"])
        .output()
        .expect("spawn the hird binary");
    let stderr = String::from_utf8_lossy(&replayed.stderr);
    assert_eq!(replayed.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        !dir.join("live.log").exists(),
        "a replayed call ran live: {}",
        fs::read_to_string(dir.join("live.log")).unwrap_or_default()
    );
    // The cursor's watcher and the crash of the call that found the cursor
    // gone race to report; either names it.
    assert!(
        stderr.contains("hird: replay cursor exited: killed") || stderr.contains("hird_replay"),
        "stderr: {stderr}"
    );
}

/// A standing program whose replay cursor is killed halts with status 1
/// and the cursor's exit reason, instead of standing on with no cursor.
#[test]
fn a_killed_replay_cursor_halts_a_standing_program() {
    if !erlang_available() {
        eprintln!("skipping: erlc not on PATH");
        return;
    }
    let dir = scratch("ffi_killed_cursor_standing");
    fs::write(
        dir.join("standing.hird"),
        "module Standing\n\nfn main() → () ! {Stand} = stand()\n",
    )
    .expect("write the program");
    fs::write(dir.join("empty.jsonl"), "").expect("write the empty log");
    let mut child = Command::new(env!("CARGO_BIN_EXE_hird"))
        .env("HIRD_CACHE_DIR", dir.join("cache"))
        .env("ERL_AFLAGS", cursor_killer(&dir, "at_rest"))
        .current_dir(&dir)
        .args([
            "run",
            "standing.hird",
            "-o",
            "out",
            "--replay",
            "empty.jsonl",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the hird binary");
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait().expect("poll the hird binary").is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the program stood on with its cursor gone");
        }
        thread::sleep(Duration::from_millis(50));
    }
    let output = child.wait_with_output().expect("collect the output");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        stderr.contains("hird: replay cursor exited: killed"),
        "stderr: {stderr}"
    );
}

/// The real-tool demo, run from the repository root as its header says:
/// the recording holds the poem it read, and replaying it succeeds.
#[test]
fn the_real_tool_demo_records_and_replays() {
    if !erlang_available() {
        eprintln!("skipping: erlc not on PATH");
        return;
    }
    let dir = scratch("ffi_demo");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let recording = dir.join("rec.jsonl");
    let run = |mode: &str| {
        Command::new(env!("CARGO_BIN_EXE_hird"))
            .env("HIRD_CACHE_DIR", dir.join("cache"))
            .current_dir(&root)
            .args(["run", "demo/real_tool/real_tool.hird", "-o"])
            .arg(dir.join("out"))
            .arg(mode)
            .arg(&recording)
            .output()
            .expect("spawn the hird binary")
    };
    let recorded = run("--audit-file");
    assert!(
        recorded.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&recorded.stderr)
    );
    let log = fs::read_to_string(&recording).expect("the recording");
    assert!(
        log.contains("\"tool\":\"ReadFile\"") && log.contains("the hird kept watch"),
        "recording: {log}"
    );
    let replayed = run("--replay");
    assert!(
        replayed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&replayed.stderr)
    );
}

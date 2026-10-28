// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The build cache, observed through `--timings=json` counters: what a build
//! compiles, what it reuses, and the emulators it boots. Every test starts
//! from an empty cache of its own. BEAM-dependent tests are skipped (with a
//! note) when `erlc` is not on the `PATH`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

/// A one-module program with a tool call, so a run writes an audit line.
const HELLO: &str = "tool Say : { message: String } -> ()\n\
     fn quiet_say(args: { message: String }) -> () = ()\n\
     fn main() -> () ! {} =\n\
       handle { Tool<Say> -> quiet_say } in say({ message: \"hello\" })\n";

/// Modules a build of [`HELLO`] produces: its base module, the 11 runtime
/// modules (the build driver among them), and the boot module.
const HELLO_MODULES: u64 = 13;

/// The runtime modules every build carries.
const RUNTIME_MODULES: u64 = 11;

/// A fresh scratch directory for one test, holding an empty `cache`.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clear scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Writes `source` as `<dir>/<name>`, creating `dir`, returning the path.
fn write(dir: &Path, name: &str, source: &str) -> String {
    fs::create_dir_all(dir).expect("create source dir");
    let path = dir.join(name);
    fs::write(&path, source).expect("write test source");
    path.display().to_string()
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

/// Runs `hird <args> --timings=json` on the build cache in `dir/cache`.
fn hird(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hird"))
        .env("HIRD_CACHE_DIR", dir.join("cache"))
        .args(args)
        .arg("--timings=json")
        .output()
        .expect("spawn the hird binary")
}

/// The `(modules_compiled, modules_reused, emulator_boots)` counters of a
/// successful command.
fn counts(output: &Output) -> (u64, u64, u64) {
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr: {err}");
    let line = err.lines().last().expect("a timings report on stderr");
    let report: Value =
        serde_json::from_str(line).unwrap_or_else(|e| panic!("not a timings report ({e}): {err}"));
    let counter = |name: &str| report["counters"][name].as_u64().expect("a counter");
    (
        counter("modules_compiled"),
        counter("modules_reused"),
        counter("emulator_boots"),
    )
}

/// The path `p` as a `&str`.
fn utf8(p: &Path) -> &str {
    p.to_str().expect("utf-8 path")
}

#[test]
fn a_second_build_boots_no_emulator() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("cache_second_build");
    let file = write(&dir, "hello.hird", HELLO);
    let (first, second) = (dir.join("first"), dir.join("second"));

    let cold = hird(&dir, &["build", &file, "-o", utf8(&first)]);
    assert_eq!(counts(&cold), (HELLO_MODULES, 0, 1));
    let warm = hird(&dir, &["build", &file, "-o", utf8(&second)]);
    assert_eq!(counts(&warm), (0, HELLO_MODULES, 0));
    let entries: Vec<String> = fs::read_dir(dir.join("cache").join("beam"))
        .expect("list the cache")
        .map(|entry| {
            entry
                .expect("cache entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(entries.len() as u64, HELLO_MODULES, "{entries:?}");
    assert!(
        entries.iter().all(|name| name.ends_with(".beam")),
        "staged copies are renamed into place: {entries:?}"
    );
    for name in [
        "hird_hello",
        "hird_boot",
        "hird_audit",
        "hird_tool_dispatch",
    ] {
        assert_eq!(
            fs::read(first.join(format!("{name}.beam"))).expect("cold beam"),
            fs::read(second.join(format!("{name}.beam"))).expect("warm beam"),
            "{name}.beam"
        );
    }

    // A run on a cache hit boots only the emulator that runs the program.
    let run = hird(&dir, &["run", &file, "-o", utf8(&second)]);
    assert_eq!(counts(&run), (0, HELLO_MODULES, 1));
}

#[test]
fn the_runtime_compiles_once() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("cache_runtime_once");
    let hello = write(&dir, "hello.hird", HELLO);
    let other = write(&dir, "other.hird", HELLO);

    let first = hird(&dir, &["build", &hello, "-o", utf8(&dir.join("a"))]);
    assert_eq!(counts(&first), (HELLO_MODULES, 0, 1));
    // Another program, `Other`, shares only the runtime: its base and boot
    // modules compile.
    let second = hird(&dir, &["build", &other, "-o", utf8(&dir.join("b"))]);
    assert_eq!(counts(&second), (2, RUNTIME_MODULES, 1));
}

#[test]
fn an_edit_recompiles_only_its_module() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("cache_edit");
    let out = dir.join("out");
    let file = write(&dir, "hello.hird", HELLO);
    assert_eq!(
        counts(&hird(&dir, &["build", &file, "-o", utf8(&out)])),
        (HELLO_MODULES, 0, 1)
    );

    // The run compiles the edited module in the emulator that runs it.
    write(&dir, "hello.hird", &HELLO.replace("hello", "goodbye"));
    let edited = hird(&dir, &["run", &file, "-o", utf8(&out)]);
    assert_eq!(counts(&edited), (1, HELLO_MODULES - 1, 1));
    assert!(
        String::from_utf8_lossy(&edited.stdout).contains("\"message\":\"goodbye\""),
        "the run must load the recompiled module: {}",
        String::from_utf8_lossy(&edited.stdout)
    );
}

#[test]
fn a_moved_source_hits_the_cache() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("cache_moved");
    let here = write(&dir.join("here"), "hello.hird", HELLO);
    let there = write(&dir.join("elsewhere").join("deeper"), "hello.hird", HELLO);

    let first = hird(&dir, &["build", &here, "-o", utf8(&dir.join("a"))]);
    assert_eq!(counts(&first), (HELLO_MODULES, 0, 1));
    let second = hird(&dir, &["build", &there, "-o", utf8(&dir.join("b"))]);
    assert_eq!(counts(&second), (0, HELLO_MODULES, 0));
}

#[test]
fn recording_and_replaying_reuse_one_build() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("cache_audit_paths");
    let file = write(&dir, "hello.hird", HELLO);
    let out = dir.join("out");
    let (recording, replayed) = (dir.join("recording.jsonl"), dir.join("replayed.jsonl"));

    let record = hird(
        &dir,
        &[
            "run",
            &file,
            "-o",
            utf8(&out),
            "--audit-file",
            utf8(&recording),
        ],
    );
    assert_eq!(counts(&record), (HELLO_MODULES, 0, 1));
    let replay = hird(
        &dir,
        &[
            "run",
            &file,
            "-o",
            utf8(&out),
            "--replay",
            utf8(&recording),
            "--audit-file",
            utf8(&replayed),
        ],
    );
    assert_eq!(counts(&replay), (0, HELLO_MODULES, 1));

    let lines = |path: &Path| {
        fs::read_to_string(path)
            .expect("read audit log")
            .lines()
            .count()
    };
    assert_eq!(lines(&recording), 1, "one tool call recorded");
    assert_eq!(lines(&replayed), 1, "the replay re-audits that call");
}

#[test]
fn built_output_runs_on_plain_erl() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("cache_plain_erl");
    let file = write(&dir, "hello.hird", HELLO);
    let out = dir.join("out");
    assert_eq!(
        counts(&hird(&dir, &["build", &file, "-o", utf8(&out)])),
        (HELLO_MODULES, 0, 1)
    );

    let audit = dir.join("audit.jsonl");
    let output = Command::new("erl")
        .args(["-noshell", "-pa", utf8(&out), "-s", "hird_boot", "run"])
        .args(["-hird_audit_file", utf8(&audit)])
        .output()
        .expect("spawn erl");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = fs::read_to_string(&audit).expect("read audit log");
    assert!(log.contains("\"tool\":\"Say\""), "audit log: {log}");
}

#[test]
fn an_unusable_cache_builds_everything() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("cache_unusable");
    let file = write(&dir, "hello.hird", HELLO);
    // A file where the cache directory would go.
    fs::write(dir.join("cache"), "").expect("write blocker");

    for out in ["a", "b"] {
        let output = hird(&dir, &["build", &file, "-o", utf8(&dir.join(out))]);
        assert_eq!(counts(&output), (HELLO_MODULES, 0, 1));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("warning: build cache off"),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn a_module_that_fails_to_compile_fails_the_command() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("cache_compile_error");
    let file = write(&dir, "hello.hird", HELLO);
    // A parse transform that does not exist fails every module's compile.
    let failing = |command: &str| {
        Command::new(env!("CARGO_BIN_EXE_hird"))
            .env("HIRD_CACHE_DIR", dir.join("cache"))
            .env(
                "ERL_COMPILER_OPTIONS",
                "[{parse_transform,no_such_transform}]",
            )
            .current_dir(&dir)
            .args([command, &file, "-o", utf8(&dir.join("out"))])
            .output()
            .expect("spawn the hird binary")
    };

    for (command, message) in [
        ("run", None),
        ("build", Some("compiling the generated Erlang failed")),
    ] {
        let output = failing(command);
        let err = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{command}: {err}");
        assert!(output.stdout.is_empty(), "{command}: stdout stays clean");
        assert!(
            err.contains("hird_hello.erl: undefined parse transform 'no_such_transform'"),
            "{command}: stderr: {err}"
        );
        if let Some(message) = message {
            assert!(err.contains(message), "{command}: stderr: {err}");
        }
    }
    let cached = fs::read_dir(dir.join("cache").join("beam"))
        .expect("list the cache")
        .count();
    assert_eq!(cached, 0, "a module that failed to compile is not cached");
    assert!(
        !dir.join("erl_crash.dump").exists(),
        "the driver reports the failure itself"
    );
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! End-to-end coverage of the supervised agent planner demo
//! (`demo/agent_planner.hird`): check, build, run, and effect graph, plus
//! the dry-run test harness — the demo with mock handlers installed in
//! place of the demo set — verified against the audit stream on stdout,
//! the golden-log regression harness replaying the checked-in recording,
//! the record-once fan-out evaluating variants of the demo against that
//! one recording, and the no-argument `hird demo` subcommand that records
//! and fans out on its own. BEAM-dependent tests are skipped (with a note)
//! when `erlc` is not on the `PATH`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The demo source checked into the repository.
fn demo_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/agent_planner.hird")
}

/// The recorded demo run checked into the repository as a golden.
fn golden_log_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/agent_planner.golden.jsonl")
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

/// The `"tool"` field of each audit line on stdout, in emission order.
fn tool_sequence(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            let rest = line.split_once("\"tool\":\"")?.1;
            Some(rest.split_once('"')?.0.to_owned())
        })
        .collect()
}

#[test]
fn demo_check_passes() {
    let output = hird(&["check", demo_path().to_str().expect("utf-8 path")]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
}

#[test]
fn demo_build_produces_actor_supervisor_and_base_modules() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("demo_build");
    let out_dir = dir.join("out");
    let output = hird(&[
        "build",
        demo_path().to_str().expect("utf-8 path"),
        "-o",
        out_dir.to_str().expect("utf-8 path"),
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    for name in [
        "hird_agent_planner.erl",
        "hird_agent_planner.beam",
        "hird_planner.erl",
        "hird_planner.beam",
        "hird_planner_sup.erl",
        "hird_planner_sup.beam",
        "hird_boot.beam",
    ] {
        assert!(out_dir.join(name).exists(), "missing {name}");
    }
}

#[test]
fn demo_runs_on_beam_and_audits_the_planning_round() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("demo_run");
    let out_dir = dir.join("out");
    let output = hird(&[
        "run",
        demo_path().to_str().expect("utf-8 path"),
        "-o",
        out_dir.to_str().expect("utf-8 path"),
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    // Nothing but the build notice on stderr: erlc warnings (unused
    // variables above all) would drown the audit stream a viewer follows.
    let err = stderr(&output);
    assert!(
        err.lines().count() == 1 && err.starts_with("compiled ") && err.contains(" module(s) to "),
        "stderr: {err}"
    );
    let out = stdout(&output);

    // One planning round, in dispatch order: the opening log, the repo
    // read, ticket-plus-log per actionable task, the closing log.
    assert_eq!(
        tool_sequence(&out),
        [
            "Log",
            "ReadRepo",
            "CreateTicket",
            "Log",
            "CreateTicket",
            "Log",
            "Log"
        ],
        "stdout: {out}"
    );
    assert!(
        out.contains("\"args\":{\"path\":{\"ctor\":\"Path\",\"args\":[\"forest-rs/hird\"]}}"),
        "stdout: {out}"
    );
    assert!(
        out.contains("\"title\":\"Fix the lexer TODO\""),
        "stdout: {out}"
    );
    assert!(
        out.contains("\"title\":\"Fuzz the parser\""),
        "stdout: {out}"
    );
    assert!(
        out.contains("\"caller\":\"Planner.handle_msg/PlanRepo\""),
        "stdout: {out}"
    );
    assert!(
        out.contains("\"caller\":\"AgentPlanner.file_tickets\""),
        "stdout: {out}"
    );
    // The priority-0 task is analyzed away: no ticket, no log line.
    let skipped: Vec<&str> = out
        .lines()
        .filter(|l| l.contains("\"tool\":\"CreateTicket\"") && l.contains("Tidy whitespace"))
        .collect();
    assert!(skipped.is_empty(), "unexpected ticket: {skipped:?}");
}

#[test]
fn demo_effect_graph_names_actor_supervisor_and_tools() {
    let path = demo_path();
    let file = path.to_str().expect("utf-8 path");

    let text = hird(&["emit-effect-graph", file]);
    assert!(text.status.success(), "stderr: {}", stderr(&text));
    let out = stdout(&text);
    assert!(out.contains("actor Planner"), "stdout: {out}");
    assert!(out.contains("supervisor PlannerSup"), "stdout: {out}");

    let json = hird(&["emit-effect-graph", file, "--json"]);
    assert!(json.status.success(), "stderr: {}", stderr(&json));
    let out = stdout(&json);
    assert!(out.contains("\"schema_version\": 1"), "stdout: {out}");
    // The Planner actor and its full effect summary.
    assert!(out.contains("\"name\": \"Planner\""), "stdout: {out}");
    for effect in ["Tool<ReadRepo>", "Tool<CreateTicket>", "Tool<Log>"] {
        assert!(
            out.contains(&format!("\"display\": \"{effect}\"")),
            "missing {effect}: {out}"
        );
    }
    // The mailbox sum type.
    for ctor in ["PlanRepo", "GetStatus", "Shutdown"] {
        assert!(
            out.contains(&format!("\"name\": \"{ctor}\"")),
            "missing {ctor}: {out}"
        );
    }
    // The supervisor, its strategy, and its child.
    assert!(out.contains("\"name\": \"PlannerSup\""), "stdout: {out}");
    assert!(out.contains("\"one_for_one\""), "stdout: {out}");
    assert!(out.contains("\"planner\""), "stdout: {out}");
    // Tool declarations with structured argument/return types.
    for tool in ["ReadRepo", "CreateTicket", "Log"] {
        assert!(
            out.contains(&format!("\"name\": \"{tool}\"")),
            "missing {tool}: {out}"
        );
    }
}

/// The dry-run harness: the demo program with the install block swapped to
/// mock handlers (canned repo data; ticket creation recorded by the audit
/// stream, not performed). Everything else — the actor, the supervisor,
/// the tools — is the demo source verbatim.
fn harness_source() -> String {
    let demo = fs::read_to_string(demo_path()).expect("read the demo source");
    let mut source = demo;
    for (demo_handler, mock_handler) in [
        ("demo_read_repo", "mock_read_repo"),
        ("demo_create_ticket", "mock_create_ticket"),
        ("demo_log", "mock_log"),
    ] {
        let arm = format!("\u{2192} {demo_handler},");
        let swapped = source.replace(&arm, &format!("\u{2192} {mock_handler},"));
        assert_ne!(swapped, source, "install arm for {demo_handler} not found");
        source = swapped;
    }
    source.push_str(
        "\nfn mock_read_repo(args: { path: Path }) \u{2192} RepoState =\n\
         \x20 RepoState(\n\
         \x20   \"mock\",\n\
         \x20   Backlog(Task(2, \"Mock: refit the keel\", \"From the mock repository.\"),\n\
         \x20   Backlog(Task(0, \"Mock: skip me\", \"Not actionable.\"),\n\
         \x20   Backlog(Task(1, \"Mock: caulk the hull\", \"From the mock repository.\"),\n\
         \x20   EmptyBacklog))))\n\
         \nfn mock_create_ticket(args: { title: String, body: String }) \u{2192} TicketId =\n\
         \x20 TicketId(\"TK-mock\")\n\
         \nfn mock_log(args: { level: String, message: String }) \u{2192} () = ()\n",
    );
    source
}

#[test]
fn harness_mocks_tools_and_verifies_the_audit_stream() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("demo_harness");
    let file = dir.join("agent_planner.hird");
    fs::write(&file, harness_source()).expect("write the harness source");
    let out_dir = dir.join("out");
    let output = hird(&[
        "run",
        file.to_str().expect("utf-8 path"),
        "-o",
        out_dir.to_str().expect("utf-8 path"),
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let out = stdout(&output);

    // The same program, the same unconditional audit stream — only the
    // installed handler set differs.
    assert_eq!(
        tool_sequence(&out),
        [
            "Log",
            "ReadRepo",
            "CreateTicket",
            "Log",
            "CreateTicket",
            "Log",
            "Log"
        ],
        "stdout: {out}"
    );
    // The mock ReadRepo handler supplied the canned repository state.
    assert!(
        out.contains("\"tool\":\"ReadRepo\"")
            && out.contains("{\"ctor\":\"RepoState\",\"args\":[\"mock\","),
        "stdout: {out}"
    );
    // Expected tickets were "created" through the mock: one invocation
    // record per actionable task, each with the mock ticket id.
    for title in ["Mock: refit the keel", "Mock: caulk the hull"] {
        assert!(
            out.lines().any(|l| l.contains("\"tool\":\"CreateTicket\"")
                && l.contains(&format!("\"title\":\"{title}\""))
                && l.contains(
                    "\"result\":{\"ok\":{\"ctor\":\"TicketId\",\"args\":[\"TK-mock\"]}}"
                )),
            "missing CreateTicket record for {title}: {out}"
        );
    }
    let skipped: Vec<&str> = out
        .lines()
        .filter(|l| l.contains("\"tool\":\"CreateTicket\"") && l.contains("Mock: skip me"))
        .collect();
    assert!(skipped.is_empty(), "unexpected ticket: {skipped:?}");
    // Progress went through Tool<Log>, audited like any other tool.
    for message in ["planning repository", "planning complete"] {
        assert!(
            out.lines().any(|l| l.contains("\"tool\":\"Log\"")
                && l.contains(&format!("\"message\":\"{message}\""))),
            "missing Log record for {message}: {out}"
        );
    }
}

/// The golden-log regression harness: the demo replayed against
/// `demo/agent_planner.golden.jsonl`, a run of the demo recorded with
/// `--audit-file`. The recorded log is the reference behavior; the
/// replay is green only while the current compiler and demo make the
/// same calls, in the same order, with the same arguments.
#[test]
fn demo_replays_the_checked_in_golden_log() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("demo_golden_replay");
    let out_dir = dir.join("out");
    let replayed = dir.join("replayed.jsonl");
    let output = hird(&[
        "run",
        demo_path().to_str().expect("utf-8 path"),
        "-o",
        out_dir.to_str().expect("utf-8 path"),
        "--replay",
        golden_log_path().to_str().expect("utf-8 path"),
        "--audit-file",
        replayed.to_str().expect("utf-8 path"),
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    // Replay consumed the whole log — `--replay` fails a run that leaves
    // records unread — and audited the recorded sequence back.
    let golden = fs::read_to_string(golden_log_path()).expect("read the golden log");
    let replayed = fs::read_to_string(&replayed).expect("read the replayed log");
    assert_eq!(
        strip_timestamps(&replayed),
        strip_timestamps(&golden),
        "the demo drifted from its recorded run"
    );
}

/// The demo with its actionability rule widened to include priority-0
/// tasks: the same program, one different agent decision.
fn drifted_source() -> String {
    let demo = fs::read_to_string(demo_path()).expect("read the demo source");
    let drifted = demo.replace("if priority > 0", "if priority >= 0");
    assert_ne!(drifted, demo, "the actionability test was not found");
    drifted
}

#[test]
fn a_drifted_demo_diverges_from_the_golden_log() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("demo_golden_divergence");
    // The module name is checked against the file name, so the drifted
    // copy keeps the demo's.
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).expect("create the source dir");
    let file = src_dir.join("agent_planner.hird");
    fs::write(&file, drifted_source()).expect("write the drifted source");
    let output = hird(&[
        "run",
        file.to_str().expect("utf-8 path"),
        "-o",
        dir.join("out").to_str().expect("utf-8 path"),
        "--replay",
        golden_log_path().to_str().expect("utf-8 path"),
    ]);
    assert!(
        !output.status.success(),
        "a drifted demo must fail its golden log"
    );

    // The first four calls match; the fifth files a ticket for the task
    // the recorded run analyzed away, and the divergence names it.
    let err = stderr(&output);
    assert!(err.contains("replay_divergence"), "stderr: {err}");
    assert!(err.contains("args_mismatch"), "stderr: {err}");
    assert!(err.contains("position => 4"), "stderr: {err}");
    assert!(err.contains("Tidy whitespace"), "stderr: {err}");
    assert!(err.contains("Fuzz the parser"), "stderr: {err}");
}

/// The demo with the ticket and its log line swapped in `file_tickets`:
/// the same tickets, announced before they are filed instead of after.
fn announce_first_source() -> String {
    let demo = fs::read_to_string(demo_path()).expect("read the demo source");
    let filing = "      let ticket = create_ticket({ title: title, body: body }) in\n";
    let announcement = "      let logged = log({ level: \"info\", message: title }) in\n";
    let swapped = demo.replace(
        &format!("{filing}{announcement}"),
        &format!("{announcement}{filing}"),
    );
    assert_ne!(swapped, demo, "the ticket and its log line were not found");
    swapped
}

/// What one arm of a fan-out made of the shared recording.
#[derive(Debug)]
enum Outcome {
    /// The arm replayed the episode to its end: the same calls, in order.
    Agreed,
    /// The arm parted from the episode.
    Parted {
        /// 0-based position of the first call that differs.
        position: usize,
        /// The `replay_divergence` report the run failed with.
        crash: String,
    },
}

/// Replays the golden episode against one variant of the demo. Every arm
/// meets the same environment — the log answers every tool call — so a
/// difference in outcome is a difference in the program, nothing else.
fn replay_arm(name: &str, source: &str) -> Outcome {
    let dir = scratch(&format!("fanout_{name}"));
    // The module name is checked against the file name, so every arm
    // keeps the demo's.
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).expect("create the source dir");
    let file = src_dir.join("agent_planner.hird");
    fs::write(&file, source).expect("write the arm source");
    let out_dir = dir.join("out");
    let output = hird(&[
        "run",
        file.to_str().expect("utf-8 path"),
        "-o",
        out_dir.to_str().expect("utf-8 path"),
        "--replay",
        golden_log_path().to_str().expect("utf-8 path"),
    ]);
    if output.status.success() {
        return Outcome::Agreed;
    }
    let crash = stderr(&output);
    let position = crash_field(&crash, "position")
        .parse()
        .expect("a numeric divergence position");
    Outcome::Parted { position, crash }
}

/// The value of `key => …` in a crash report, read to the first character
/// that ends an integer or an atom.
fn crash_field<'a>(crash: &'a str, key: &str) -> &'a str {
    let rest = crash
        .split_once(&format!("{key} => "))
        .unwrap_or_else(|| panic!("no `{key} =>` in the crash report: {crash}"))
        .1;
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    &rest[..end]
}

/// One arm's line in the fan-out summary, against an episode of `calls`.
fn verdict(outcome: &Outcome, calls: usize) -> String {
    match outcome {
        Outcome::Agreed => format!("agreed with all {calls} calls"),
        Outcome::Parted { position, crash } => {
            format!("parted at call {position} ({})", crash_field(crash, "kind"))
        }
    }
}

/// The record-once fan-out: one recorded episode, several variants of the
/// program, one shared axis. Replay serves every tool result from the
/// log, so the environment is identical across arms and the program is
/// the only variable left; strict-sequential matching then places each
/// arm on the recording's own positions, which makes the arms comparable
/// with each other and not just with the recording.
#[test]
fn one_recording_fans_out_over_demo_variants() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let demo = fs::read_to_string(demo_path()).expect("read the demo source");
    let episode = fs::read_to_string(golden_log_path()).expect("read the golden log");
    let calls = episode.lines().count();

    let baseline = replay_arm("baseline", &demo);
    let announce_first = replay_arm("announce-first", &announce_first_source());
    let eager = replay_arm("eager", &drifted_source());

    // The evaluation's output: where each arm's decisions leave the
    // recorded episode. The baseline is the control — it is the program
    // the episode was recorded from, so it replays green and every
    // departure below it belongs to an edit, not to the harness.
    let summary = [
        ("baseline", &baseline),
        ("announce-first", &announce_first),
        ("eager", &eager),
    ]
    .iter()
    .map(|(name, outcome)| format!("{name:<14}  {}", verdict(outcome, calls)))
    .collect::<Vec<String>>()
    .join("\n");
    println!("{summary}");
    assert_eq!(
        summary,
        "baseline        agreed with all 7 calls\n\
         announce-first  parted at call 2 (tool_mismatch)\n\
         eager           parted at call 4 (args_mismatch)",
        "the fan-out summary"
    );

    // Read across the shared axis: both variants make the episode's first
    // two calls, so that is as far as they agree with each other. At call
    // 2 the episode files a ticket — which the eager arm still matched —
    // and the announce-first arm logs instead.
    let Outcome::Parted { position, crash } = &announce_first else {
        panic!("the announce-first arm must part from the episode");
    };
    assert_eq!(
        tool_sequence(&episode)[*position],
        "CreateTicket",
        "the episode's call at the parting position"
    );
    assert!(
        crash.contains("tool => create_ticket"),
        "the expected call: {crash}"
    );
    assert!(crash.contains("tool => log"), "the offered call: {crash}");
}

/// The no-argument demonstration: `hird demo` writes the embedded planner
/// and its variants into a directory of its own, records one run of the
/// planner, replays that recording against every variant, and prints the
/// divergence table. Nothing about the invocation names a file, and the
/// episode each arm is evaluated against is the one the command just
/// recorded.
#[test]
fn the_demo_subcommand_records_a_run_and_prints_the_divergence_table() {
    if !erlang_available() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = scratch("demo_subcommand");
    let out_dir = dir.join("out");
    let out_arg = out_dir.to_str().expect("utf-8 path").to_owned();

    let output = hird(&["demo", "-o", &out_arg]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let table = "  baseline        agreed with all 7 calls\n\
                 \x20 announce-first  parted at call 2 (tool_mismatch)\n\
                 \x20 eager           parted at call 4 (args_mismatch)\n";
    let out = stdout(&output);
    assert!(out.contains(table), "stdout: {out}");

    // The episode came from this run rather than from the tree — and it
    // is the checked-in recording again, timestamps aside.
    let recording =
        fs::read_to_string(out_dir.join("recording.jsonl")).expect("read the recording");
    let golden = fs::read_to_string(golden_log_path()).expect("read the golden log");
    assert_eq!(
        strip_timestamps(&recording),
        strip_timestamps(&golden),
        "the recorded episode"
    );

    // Running again re-records: the audit sink appends, so a stale
    // episode would otherwise be replayed along with the new one.
    let again = hird(&["demo", "-o", &out_arg]);
    assert!(again.status.success(), "stderr: {}", stderr(&again));
    let out = stdout(&again);
    assert!(out.contains(table), "stdout: {out}");
}

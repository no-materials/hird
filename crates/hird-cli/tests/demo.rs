// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! End-to-end coverage of the supervised agent planner demo
//! (`demo/agent_planner.hird`): check, build, run, and effect graph, plus
//! the dry-run test harness — the demo with mock handlers installed in
//! place of the demo set — verified against the audit stream on stdout.
//! BEAM-dependent tests are skipped (with a note) when `erlc` is not on
//! the `PATH`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The demo source checked into the repository.
fn demo_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/agent_planner.hird")
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

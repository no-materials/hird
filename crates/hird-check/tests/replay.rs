// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Replay-driver integration tests: the golden planner log replayed green
//! against the demo tool signatures, divergence reports rendered
//! actionably, and the property that any recorded run replays green
//! against itself.

use hird_ast::{AstNode, SourceFile};
use hird_check::CheckedFile;
use hird_check::replay::{ReplayCursor, ToolTable, load_log};
use hird_check::wire::{
    AdtTable, AuditSink, InvocationRecord, JsonLinesSink, ToolResult, WireValue,
};
use proptest::prelude::*;

/// The planner demo's tool declarations, which the golden log's records
/// draw their signatures from.
const PLANNER_TOOLS: &str = "\
effect Tool<t>
effect Exn<t>
type TicketId = TicketId(String)
type HttpError = HttpError(Int, String)
tool ReadRepo : { path: String } -> { files: List<String>, status: String }
tool CreateTicket : { title: String, body: String } -> TicketId
tool HttpGet : { url: String } -> String ! {Exn<HttpError>}
";

/// The golden planner log's raw JSON lines.
const GOLDEN_LOG: &str = include_str!("../../../conformance/v1/planner_log.jsonl");

/// Parses and checks `source`, asserting it is parse-error-free.
fn check(source: &str) -> CheckedFile {
    let parsed = hird_parse::parse(source, 0);
    assert!(
        parsed.is_ok(),
        "test source has parse errors: {:?}",
        parsed.diagnostics()
    );
    let file = SourceFile::cast(parsed.syntax().clone()).expect("root is a source file");
    hird_check::check(&file, 0)
}

/// The planner demo's tool and ADT tables.
fn planner_context() -> (ToolTable, AdtTable) {
    let checked = check(PLANNER_TOOLS);
    assert!(!checked.has_errors(), "planner tools must check cleanly");
    (
        ToolTable::from_checked(&checked),
        AdtTable::from_checked(&checked),
    )
}

// ── the golden log ──────────────────────────────────────────────

/// The driver replays the golden log green: every in-order offer returns
/// its logged result — the HTTP failure included — and consumes the log.
#[test]
fn the_golden_log_replays_green() {
    let (tools, adts) = planner_context();
    let log = load_log(GOLDEN_LOG, &tools, &adts).expect("the golden log loads");
    let mut cursor = ReplayCursor::new(&log);
    for record in &log {
        let result = cursor
            .offer(&record.tool, &record.args)
            .expect("in-order replay is green");
        assert_eq!(result, &record.result);
    }
    assert_eq!(cursor.remaining(), 0, "the whole log is consumed");
}

// ── divergence reports ──────────────────────────────────────────

/// The golden log, decoded, with a cursor already driven `consumed`
/// records in.
fn cursor_at(log: &[InvocationRecord], consumed: usize) -> ReplayCursor<'_> {
    let mut cursor = ReplayCursor::new(log);
    for record in &log[..consumed] {
        cursor
            .offer(&record.tool, &record.args)
            .expect("the prefix replays green");
    }
    cursor
}

#[test]
fn an_exhausted_log_renders_the_offered_call() {
    let (tools, adts) = planner_context();
    let log = load_log(GOLDEN_LOG, &tools, &adts).expect("the golden log loads");
    let mut cursor = cursor_at(&log, log.len());
    let report = cursor
        .offer("ReadRepo", &log[0].args)
        .expect_err("the log is exhausted");
    insta::assert_snapshot!(report);
}

#[test]
fn a_tool_mismatch_renders_expected_and_offered_calls() {
    let (tools, adts) = planner_context();
    let log = load_log(GOLDEN_LOG, &tools, &adts).expect("the golden log loads");
    let mut cursor = cursor_at(&log, 0);
    let args = WireValue::record([
        ("body", WireValue::string("Investigate flaky CI on main")),
        ("title", WireValue::string("Flaky CI")),
    ]);
    let report = cursor
        .offer("CreateTicket", &args)
        .expect_err("the log expects ReadRepo");
    insta::assert_snapshot!(report);
}

#[test]
fn an_args_mismatch_renders_both_args() {
    let (tools, adts) = planner_context();
    let log = load_log(GOLDEN_LOG, &tools, &adts).expect("the golden log loads");
    let mut cursor = cursor_at(&log, 0);
    let args = WireValue::record([("path", WireValue::string("/other/repo"))]);
    let report = cursor
        .offer("ReadRepo", &args)
        .expect_err("the args differ");
    insta::assert_snapshot!(report);
}

// ── recorded runs replay against themselves ─────────────────────

/// A record with the fixed envelope fields around `tool`, `args`, `result`.
fn record(tool: &str, args: WireValue, result: ToolResult) -> InvocationRecord {
    InvocationRecord {
        tool: String::from(tool),
        args,
        result,
        timestamp: String::from("2026-05-22T12:00:00.000Z"),
        caller: String::from("Planner.plan_repo"),
        meta: None,
    }
}

/// A recorded `ReadRepo` invocation with arbitrary args and result.
fn arb_read_repo() -> impl Strategy<Value = InvocationRecord> {
    (
        any::<String>(),
        prop::collection::vec(any::<String>(), 0..3),
        any::<String>(),
    )
        .prop_map(|(path, files, status)| {
            record(
                "ReadRepo",
                WireValue::record([("path", WireValue::string(path))]),
                ToolResult::Ok(WireValue::record([
                    (
                        "files",
                        WireValue::List(files.into_iter().map(WireValue::string).collect()),
                    ),
                    ("status", WireValue::string(status)),
                ])),
            )
        })
}

/// A recorded `CreateTicket` invocation with arbitrary args and result.
fn arb_create_ticket() -> impl Strategy<Value = InvocationRecord> {
    (any::<String>(), any::<String>(), any::<String>()).prop_map(|(title, body, id)| {
        record(
            "CreateTicket",
            WireValue::record([
                ("title", WireValue::string(title)),
                ("body", WireValue::string(body)),
            ]),
            ToolResult::Ok(WireValue::ctor("TicketId", vec![WireValue::string(id)])),
        )
    })
}

/// A recorded `HttpGet` invocation: an ok body or a declared `HttpError`.
fn arb_http_get() -> impl Strategy<Value = InvocationRecord> {
    let ok = any::<String>().prop_map(|body| ToolResult::Ok(WireValue::string(body)));
    let err = (any::<i64>(), any::<String>()).prop_map(|(code, message)| {
        ToolResult::Err(WireValue::ctor(
            "HttpError",
            vec![WireValue::Int(code), WireValue::string(message)],
        ))
    });
    (any::<String>(), prop_oneof![ok, err]).prop_map(|(url, result)| {
        record(
            "HttpGet",
            WireValue::record([("url", WireValue::string(url))]),
            result,
        )
    })
}

/// One arbitrary planner-tool invocation record.
fn arb_record() -> impl Strategy<Value = InvocationRecord> {
    prop_oneof![arb_read_repo(), arb_create_ticket(), arb_http_get()]
}

proptest! {
    /// Any recorded run replays green against itself: encode the run with
    /// the default sink, load it back against the tool signatures, and
    /// every offered call in order returns its logged result with nothing
    /// left over.
    #[test]
    fn recorded_runs_replay_green(records in prop::collection::vec(arb_record(), 0..8)) {
        let (tools, adts) = planner_context();
        let mut sink = JsonLinesSink::new();
        for record in &records {
            sink.emit(record).expect("recorded runs encode");
        }
        let log = load_log(sink.as_str(), &tools, &adts).expect("a recorded run loads");
        prop_assert_eq!(&log, &records);
        let mut cursor = ReplayCursor::new(&log);
        for record in &records {
            match cursor.offer(&record.tool, &record.args) {
                Ok(result) => prop_assert_eq!(result, &record.result),
                Err(report) => prop_assert!(false, "in-order replay diverged: {}", report),
            }
        }
        prop_assert_eq!(cursor.remaining(), 0);
    }
}

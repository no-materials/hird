// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Wire-format integration tests: byte-exact golden conformance files, the
//! audit-sink capability fixtures, strict-sequential replay, and the
//! encode/decode round-trip property.

use std::collections::BTreeMap;
use std::fmt::Write;

use hird_ast::{AstNode, SourceFile};
use hird_check::wire::{
    AdtTable, AuditSink, InvocationRecord, JsonLinesSink, MetaValue, ToolResult, ToolWireSig,
    WireValue, decode_record, encode_record, encode_value, replay,
};
use hird_check::{CheckedFile, Severity};
use hird_types::{Label, Name, Type};
use proptest::prelude::*;

/// The planner demo's tool declarations, which the golden records draw
/// their signatures from.
const PLANNER_TOOLS: &str = "\
type TicketId = TicketId(String)
type HttpError = HttpError(Int, String)
tool ReadRepo : { path: String } -> { files: List<String>, status: String }
tool CreateTicket : { title: String, body: String } -> TicketId
tool HttpGet : { url: String } -> String ! {Exn<HttpError>}
";

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

/// Renders a checked file as bindings, invocation records, and diagnostics.
fn render(checked: &CheckedFile) -> String {
    let mut out = String::new();
    for (name, ty) in &checked.bindings {
        writeln!(out, "{name} : {}", ty.normalized()).unwrap();
    }
    for (name, ty) in &checked.invocation_records {
        writeln!(out, "record {name} : {}", ty.normalized()).unwrap();
    }
    for diag in &checked.diagnostics {
        let severity = match diag.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        writeln!(
            out,
            "{severity}[{:?}] {}..{}: {}",
            diag.code, diag.span.start, diag.span.end, diag.message
        )
        .unwrap();
    }
    out
}

/// The wire signature of the tool bound as `tool_fn`.
fn sig(checked: &CheckedFile, tool_fn: &str) -> ToolWireSig {
    let scheme = checked
        .bindings
        .get(tool_fn)
        .expect("tool function is bound");
    ToolWireSig::from_fn(scheme).expect("tool functions are unary")
}

// ── golden conformance files ────────────────────────────────────

/// The record `conformance/v1/read_repo_ok.json` snapshots.
fn read_repo_record() -> InvocationRecord {
    InvocationRecord {
        tool: String::from("ReadRepo"),
        args: WireValue::record([("path", WireValue::string("/home/user/repo"))]),
        result: ToolResult::Ok(WireValue::record([
            ("files", WireValue::List(vec![])),
            ("status", WireValue::string("clean")),
        ])),
        timestamp: String::from("2026-05-22T12:00:00.000Z"),
        caller: String::from("Planner.plan_repo"),
        meta: Some(BTreeMap::from([(
            String::from("duration_ms"),
            MetaValue::Int(42),
        )])),
    }
}

/// The record `conformance/v1/create_ticket_ok.json` snapshots.
fn create_ticket_record() -> InvocationRecord {
    InvocationRecord {
        tool: String::from("CreateTicket"),
        args: WireValue::record([
            ("title", WireValue::string("Flaky CI")),
            ("body", WireValue::string("Investigate flaky CI on main")),
        ]),
        result: ToolResult::Ok(WireValue::ctor(
            "TicketId",
            vec![WireValue::string("TCK-42")],
        )),
        timestamp: String::from("2026-05-22T12:00:01.250Z"),
        caller: String::from("Planner.plan_repo"),
        meta: None,
    }
}

/// The record `conformance/v1/http_get_err.json` snapshots: a failed
/// invocation, first-class on the wire.
fn http_get_err_record() -> InvocationRecord {
    InvocationRecord {
        tool: String::from("HttpGet"),
        args: WireValue::record([("url", WireValue::string("https://ci.example/status"))]),
        result: ToolResult::Err(WireValue::ctor(
            "HttpError",
            vec![
                WireValue::Int(503),
                WireValue::string("service unavailable"),
            ],
        )),
        timestamp: String::from("2026-05-22T12:00:02.000Z"),
        caller: String::from("Planner.check_ci"),
        meta: Some(BTreeMap::from([(
            String::from("duration_ms"),
            MetaValue::Int(1200),
        )])),
    }
}

/// Asserts `record` reproduces `golden` byte for byte, and that the golden
/// line decodes against the tool's signature back to `record`.
#[track_caller]
fn assert_golden(record: &InvocationRecord, tool_fn: &str, golden: &str) {
    let checked = check(PLANNER_TOOLS);
    assert!(!checked.has_errors(), "planner tools must check cleanly");
    let line = encode_record(record).expect("golden records encode");
    assert_eq!(format!("{line}\n"), golden, "encoding must be byte-exact");
    let sig = sig(&checked, tool_fn);
    let adts = AdtTable::from_checked(&checked);
    let decoded = decode_record(golden.trim_end(), &sig, &adts).expect("golden line decodes");
    assert_eq!(&decoded, record, "decode inverts encode");
}

#[test]
fn golden_read_repo_ok() {
    assert_golden(
        &read_repo_record(),
        "read_repo",
        include_str!("../../../conformance/v1/read_repo_ok.json"),
    );
}

#[test]
fn golden_create_ticket_ok() {
    assert_golden(
        &create_ticket_record(),
        "create_ticket",
        include_str!("../../../conformance/v1/create_ticket_ok.json"),
    );
}

#[test]
fn golden_http_get_err() {
    assert_golden(
        &http_get_err_record(),
        "http_get",
        include_str!("../../../conformance/v1/http_get_err.json"),
    );
}

/// The default sink writes a sequence of tool calls as JSON lines,
/// byte-identical to the golden log.
#[test]
fn golden_audit_log_sequence() {
    let mut sink = JsonLinesSink::new();
    for record in [
        read_repo_record(),
        create_ticket_record(),
        http_get_err_record(),
    ] {
        sink.emit(&record).expect("golden records encode");
    }
    assert_eq!(
        sink.as_str(),
        include_str!("../../../conformance/v1/planner_log.jsonl"),
        "the log must be byte-exact"
    );
}

// ── the audit sink as a capability ──────────────────────────────

/// The audit sink threads explicitly: emission takes the sink parameter,
/// and the audited handler's `Audit<AuditSink>` effect stays visible in
/// the effect row of the function installing it.
#[test]
fn audit_sink_is_a_capability() {
    insta::assert_snapshot!(render(&check(include_str!("fixtures/audit_sink.hird"))));
}

/// Omitting the sink parameter fails to type-check: there is no ambient
/// sink to emit to.
#[test]
fn omitting_the_sink_fails_to_type_check() {
    insta::assert_snapshot!(render(&check(
        "effect Audit<t>\n\
         type Path = Path(String)\n\
         type RepoState = RepoState(String)\n\
         type AuditSink = AuditSink(String)\n\
         tool ReadRepo : { path: Path } -> RepoState\n\
         fn audited_read(\n\
           emit: { sink: AuditSink, line: String } -> () ! {Audit<sink>},\n\
           args: { path: Path }\n\
         ) -> RepoState ! {Audit<sink>, Tool<ReadRepo>} =\n\
           let logged = emit({ sink: sink, line: \"ReadRepo\" }) in read_repo(args)"
    )));
}

// ── replay ──────────────────────────────────────────────────────

/// The golden log, decoded record by record against each tool's signature.
fn planner_log() -> Vec<InvocationRecord> {
    let checked = check(PLANNER_TOOLS);
    let adts = AdtTable::from_checked(&checked);
    let lines: Vec<&str> = include_str!("../../../conformance/v1/planner_log.jsonl")
        .lines()
        .collect();
    let tool_fns = ["read_repo", "create_ticket", "http_get"];
    lines
        .iter()
        .zip(tool_fns)
        .map(|(line, tool_fn)| {
            decode_record(line, &sig(&checked, tool_fn), &adts).expect("golden line decodes")
        })
        .collect()
}

/// Replaying the golden log in order returns every logged result — the
/// failure included — without re-executing anything.
#[test]
fn replay_returns_logged_values() {
    let log = planner_log();
    let mut out = String::new();
    for (position, record) in log.iter().enumerate() {
        let result = replay(&log, position, &record.tool, &record.args).expect("in-order replay");
        let rendered = match result {
            ToolResult::Ok(value) => format!("ok {}", encode_value(value).unwrap()),
            ToolResult::Err(value) => format!("err {}", encode_value(value).unwrap()),
        };
        writeln!(out, "{position} {} -> {rendered}", record.tool).unwrap();
    }
    insta::assert_snapshot!(out);
}

/// Every divergence is a hard, structured error: wrong tool, same tool with
/// different args, and an exhausted log.
#[test]
fn replay_divergence_is_a_structured_error() {
    let log = planner_log();
    let wrong_tool = replay(&log, 0, "CreateTicket", &log[0].args).unwrap_err();
    let other_args = WireValue::record([("path", WireValue::string("/other/repo"))]);
    let wrong_args = replay(&log, 0, "ReadRepo", &other_args).unwrap_err();
    let exhausted = replay(&log, 3, "ReadRepo", &log[0].args).unwrap_err();
    let mut out = String::new();
    writeln!(out, "{wrong_tool}").unwrap();
    writeln!(out, "{wrong_args}").unwrap();
    writeln!(out, "{wrong_args:?}").unwrap();
    writeln!(out, "{exhausted}").unwrap();
    insta::assert_snapshot!(out);
}

// ── round-trip property ─────────────────────────────────────────

/// The ADTs the generated values draw from: `Bool` and a generic `Opt`.
fn prop_adts() -> AdtTable {
    let mut table = AdtTable::new();
    table.insert(
        Name::new("Bool"),
        vec![(Name::new("True"), vec![]), (Name::new("False"), vec![])],
    );
    table.insert(
        Name::new("Opt"),
        vec![
            (Name::new("Some"), vec![Type::TyVar(0)]),
            (Name::new("None"), vec![]),
        ],
    );
    table
}

/// A random wire-representable type.
fn arb_type() -> impl Strategy<Value = Type> {
    let leaf = prop_oneof![
        Just(Type::int()),
        Just(Type::float()),
        Just(Type::string()),
        Just(Type::bool()),
        Just(Type::tuple(vec![])),
    ];
    leaf.prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            inner.clone().prop_map(Type::list),
            prop::collection::vec(inner.clone(), 1..3).prop_map(Type::tuple),
            prop::collection::vec(inner.clone(), 1..3).prop_map(|tys| {
                Type::record(
                    tys.into_iter()
                        .enumerate()
                        .map(|(i, ty)| (Label::new(format!("f{i}")), ty)),
                )
            }),
            inner.prop_map(|ty| Type::con("Opt", vec![ty])),
        ]
    })
}

/// A random value of type `ty`.
fn arb_value(ty: &Type) -> BoxedStrategy<WireValue> {
    match ty {
        Type::TyCon(name, args) => match (name.as_str(), args.as_slice()) {
            ("Int", []) => any::<i64>().prop_map(WireValue::Int).boxed(),
            ("Float", []) => any::<f64>()
                .prop_filter("finite floats only", |f| f.is_finite())
                .prop_map(WireValue::Float)
                .boxed(),
            ("String", []) => any::<String>().prop_map(WireValue::String).boxed(),
            ("Bool", []) => prop_oneof![
                Just(WireValue::ctor("True", vec![])),
                Just(WireValue::ctor("False", vec![])),
            ]
            .boxed(),
            ("List", [elem]) => prop::collection::vec(arb_value(elem), 0..4)
                .prop_map(WireValue::List)
                .boxed(),
            ("Opt", [inner]) => {
                let some = arb_value(inner).prop_map(|v| WireValue::ctor("Some", vec![v]));
                prop_oneof![Just(WireValue::ctor("None", vec![])), some].boxed()
            }
            _ => unreachable!("arb_type generates no other constructors"),
        },
        Type::TyTuple(elems) if elems.is_empty() => Just(WireValue::Unit).boxed(),
        Type::TyTuple(elems) => elems
            .iter()
            .map(arb_value)
            .collect::<Vec<_>>()
            .prop_map(WireValue::Tuple)
            .boxed(),
        Type::TyRecord(fields) => {
            let labels: Vec<Label> = fields.keys().cloned().collect();
            fields
                .values()
                .map(arb_value)
                .collect::<Vec<_>>()
                .prop_map(move |values| {
                    WireValue::Record(labels.iter().cloned().zip(values).collect())
                })
                .boxed()
        }
        _ => unreachable!("arb_type generates no other shapes"),
    }
}

proptest! {
    /// Every well-typed value decodes back from its canonical encoding, and
    /// re-encoding is byte-identical.
    #[test]
    fn value_round_trips(
        (ty, value) in arb_type().prop_flat_map(|ty| {
            let value = arb_value(&ty);
            (Just(ty), value)
        })
    ) {
        let adts = prop_adts();
        let encoded = encode_value(&value).expect("finite values encode");
        let decoded = hird_check::wire::decode_value(&encoded, &ty, &adts)
            .expect("canonical encoding decodes");
        prop_assert_eq!(&decoded, &value);
        prop_assert_eq!(encode_value(&decoded).expect("finite values encode"), encoded);
    }
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Record update: `{ f: v, ..base }` keeps the base's type, needs a known
//! record base carrying every listed field, and is never field-less.

use std::fmt::Write;

use hird_ast::{AstNode, SourceFile};
use hird_check::Severity;

/// Parses, checks, and renders `source` as resolved top-level bindings,
/// derived invocation records, and diagnostics.
fn check_str(source: &str) -> String {
    let parsed = hird_parse::parse(source, 0);
    assert!(
        parsed.is_ok(),
        "test source has parse errors: {:?}",
        parsed.diagnostics()
    );
    let file = SourceFile::cast(parsed.syntax().clone()).expect("root is a source file");
    let checked = hird_check::check(&file, 0);
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

/// The listed fields come from the literal, the rest from the base; the
/// result has the base's type, so an update with all, some, or one field is
/// the same record.
#[test]
fn update_keeps_the_base_type() {
    insta::assert_snapshot!(check_str(
        "type alias Point = { x: Int, y: Int, label: String }\n\
         fn move_x(p: Point, dx: Int) -> Point = { x: p.x + dx, ..p }\n\
         fn relabel(p: Point) -> Point = { label: \"moved\", x: 0, ..p }\n\
         fn nested(p: Point) -> Point = { y: { x: 1, ..p }.x, ..p }"
    ));
}

/// The ADR-029 heartbeat handler: record-shaped state read by field and
/// rebuilt with `..st`.
#[test]
fn heartbeat_handler_with_record_state() {
    insta::assert_snapshot!(check_str(
        "type alias LogArgs = { message: String }\n\
         tool Log : LogArgs -> ()\n\
         type alias HeartState = { clock: Clock, period: Int, beats: Int }\n\
         actor Heart {\n\
           state: HeartState,\n\
           message: HeartMsg = | Beat,\n\
           init: fn(clock: Clock) ! {Schedule<HeartMsg>} =\n\
             schedule(clock, self(), Beat, 1000);\n\
             { clock: clock, period: 1000, beats: 0 },\n\
           handle Beat, st ! {Tool<Log>, Schedule<HeartMsg>} =\n\
             log({ message: \"beat\" });\n\
             schedule(st.clock, self(), Beat, st.period);\n\
             Continue({ beats: st.beats + 1, ..st }),\n\
         } ! {Tool<Log>, Schedule<HeartMsg>}"
    ));
}

/// Records are closed: a listed field the base lacks is C0010, and a listed
/// value must have the base field's type.
#[test]
fn update_cannot_add_or_retype_fields() {
    insta::assert_snapshot!(check_str(
        "fn add(p: { x: Int }) -> { x: Int } = { z: 1, ..p }\n\
         fn retype(p: { x: Int }) -> { x: Int } = { x: \"one\", ..p }"
    ));
}

/// The base must resolve to a record: an unannotated base needs a type
/// annotation, and a non-record base is not updatable.
#[test]
fn update_base_must_be_a_known_record() {
    insta::assert_snapshot!(check_str(
        "fn loose(p: a) = { x: 1, ..p }\n\
         fn tuple() -> Int = { x: 1, ..(1, 2) }.x"
    ));
}

/// `{ ..base }` alone is an error, not a copy.
#[test]
fn base_only_update_is_rejected() {
    insta::assert_snapshot!(check_str("fn copy(p: { x: Int }) -> { x: Int } = { ..p }"));
}

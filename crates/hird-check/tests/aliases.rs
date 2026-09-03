// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Type aliases: expansion at elaboration, arity, cycles, transparency to
//! tools and the wire format, and export through the module interface.

use std::fmt::Write;

use hird_ast::{AstNode, SourceFile};
use hird_check::{CheckedProgram, ModuleName, Severity, check_program};

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

/// Parses each `(module name, source)` pair, checks the program, and renders
/// every module's bindings followed by its diagnostics.
fn check_modules(modules: &[(&str, &str)]) -> String {
    let files: Vec<(ModuleName, SourceFile)> = modules
        .iter()
        .map(|(name, src)| {
            let parsed = hird_parse::parse(src, 0);
            assert!(
                parsed.is_ok(),
                "module `{name}` has parse errors: {:?}",
                parsed.diagnostics()
            );
            (
                ModuleName::new(*name),
                SourceFile::cast(parsed.syntax().clone()).expect("root is a source file"),
            )
        })
        .collect();
    let program: CheckedProgram = check_program(&files);
    let mut out = String::new();
    for (name, checked) in &program.modules {
        writeln!(out, "== {name} ==").unwrap();
        for (binding, ty) in &checked.bindings {
            writeln!(out, "{binding} : {}", ty.normalized()).unwrap();
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
    }
    out
}

// ── expansion ───────────────────────────────────────────────────

/// A record, a tuple, and a function type can each be aliased; every use
/// elaborates to the expansion, so the bindings show the shapes, not the
/// names.
#[test]
fn record_tuple_and_function_aliases_expand() {
    insta::assert_snapshot!(check_str(
        "type alias LogArgs = { level: String, message: String }\n\
         type alias Pair = (Int, String)\n\
         type alias Step = Int -> Int\n\
         fn level(a: LogArgs) -> String = a.level\n\
         fn swap(p: Pair) -> (String, Int) = let (n, s) = p in (s, n)\n\
         fn twice(f: Step, x: Int) -> Int = f(f(x))"
    ));
}

/// An alias has no identity: two aliases of one shape, and the shape written
/// out, are the same type.
#[test]
fn aliases_are_transparent_to_unification() {
    insta::assert_snapshot!(check_str(
        "type alias A = { x: Int }\n\
         type alias B = { x: Int }\n\
         fn a_to_b(a: A) -> B = a\n\
         fn b_to_plain(b: B) -> { x: Int } = b\n\
         fn plain_to_a() -> A = { x: 1 }"
    ));
}

/// A parametric alias substitutes its arguments; the arity is checked at
/// every use under the ADT arity diagnostic.
#[test]
fn parametric_alias_is_arity_checked() {
    insta::assert_snapshot!(check_str(
        "type alias Box<a> = { value: a }\n\
         fn unbox(b: Box<Int>) -> Int = b.value\n\
         fn bare(b: Box) -> Int = 0\n\
         fn over(b: Box<Int, Int>) -> Int = 0"
    ));
}

/// Aliases compose: an alias body may apply another alias, at any argument,
/// in any declaration order.
#[test]
fn aliases_compose_in_any_order() {
    insta::assert_snapshot!(check_str(
        "type alias IntPair = Pair<Int, Int>\n\
         type alias Wrap<a> = Pair<a, Box<a>>\n\
         type alias Pair<a, b> = (a, b)\n\
         type alias Box<a> = { value: a }\n\
         fn first(p: IntPair) -> Int = let (a, b) = p in a\n\
         fn wrap(x: String) -> Wrap<String> = (x, { value: x })"
    ));
}

/// An alias is a legal constructor field and a legal effect-row carrier: the
/// expansion lands wherever a type expression would.
#[test]
fn alias_in_constructor_field_and_handler_type() {
    insta::assert_snapshot!(check_str(
        "tool Log : { message: String } -> ()\n\
         type alias Args = { message: String }\n\
         type alias Handler = Args -> () ! {Tool<Log>}\n\
         type Msg = Do(Args)\n\
         fn run(h: Handler) -> () ! {Tool<Log>} = h({ message: \"x\" })\n\
         fn payload(m: Msg) -> String = match m { Do(a) -> a.message, }"
    ));
}

// ── rejection ───────────────────────────────────────────────────

/// A self-referential alias is a cycle, reported once at the mention that
/// closes it.
#[test]
fn self_referential_alias_rejected() {
    insta::assert_snapshot!(check_str(
        "type alias Chain = List<Chain>\n\
         fn f(c: Chain) -> Int = 0"
    ));
}

/// Mutual recursion through two aliases is one cycle: one C0059, and the
/// other alias fails silently instead of reporting the same cycle again.
/// Declarations that do not touch the cycle still check.
#[test]
fn mutually_recursive_aliases_rejected_once() {
    insta::assert_snapshot!(check_str(
        "type alias A = { b: B }\n\
         type alias B = { a: A }\n\
         fn f(a: A) -> Int = 0\n\
         fn g(x: Int) -> Int = x"
    ));
}

/// An alias shares the type namespace with ADTs and other aliases.
#[test]
fn alias_duplicates_are_reported() {
    insta::assert_snapshot!(check_str(
        "type Flag = On | Off\n\
         type alias Flag = Int\n\
         type alias Pair = (Int, Int)\n\
         type alias Pair = (String, String)"
    ));
}

/// The alias body is a closed scope: only declared parameters are in scope,
/// and a parameter may be declared once.
#[test]
fn alias_body_is_closed_over_its_parameters() {
    insta::assert_snapshot!(check_str(
        "type alias Loose = { v: a }\n\
         type alias Twice<a, a> = (a, a)\n\
         fn f(t: Twice<Int, Int>) -> Int = 0"
    ));
}

// ── tools and the wire ──────────────────────────────────────────

/// A tool typed over an alias derives the same invocation record as one
/// whose argument record is written out, so the audit wire format is
/// unchanged.
#[test]
fn tool_over_alias_derives_identical_invocation_record() {
    let written = check_str(
        "tool Log : { level: String, message: String } -> ()\n\
         fn quiet(args: { level: String, message: String }) -> () = ()",
    );
    let aliased = check_str(
        "type alias LogArgs = { level: String, message: String }\n\
         tool Log : LogArgs -> ()\n\
         fn quiet(args: LogArgs) -> () = ()",
    );
    assert_eq!(written, aliased);
    insta::assert_snapshot!(aliased);
}

/// The wire check sees through an alias: an aliased function type in a tool
/// signature is still not wire-representable.
#[test]
fn wire_check_sees_through_alias() {
    insta::assert_snapshot!(check_str(
        "type alias Thunk = () -> Int\n\
         tool Run : { f: Thunk } -> Int"
    ));
}

// ── modules ─────────────────────────────────────────────────────

/// `pub type alias` is importable by name; the importer sees the expansion,
/// including a parametric one applied at the use site.
#[test]
fn pub_alias_importable_across_modules() {
    insta::assert_snapshot!(check_modules(&[
        (
            "Shapes",
            "module Shapes\n\
             pub type alias Point = { x: Int, y: Int }\n\
             pub type alias Box<a> = { value: a }",
        ),
        (
            "App",
            "module App\n\
             use Shapes.{Point, Box}\n\
             pub fn origin() -> Point = { x: 0, y: 0 }\n\
             pub fn unbox(b: Box<Point>) -> Int = b.value.x",
        ),
    ]));
}

/// A private alias is not part of the interface.
#[test]
fn private_alias_not_importable() {
    insta::assert_snapshot!(check_modules(&[
        (
            "Shapes",
            "module Shapes\ntype alias Point = { x: Int, y: Int }"
        ),
        (
            "App",
            "module App\nuse Shapes.{Point}\npub fn origin() -> Point = { x: 0, y: 0 }",
        ),
    ]));
}

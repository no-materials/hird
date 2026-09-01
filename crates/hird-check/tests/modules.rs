// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Whole-program (multi-module) checking: import resolution, qualified names,
//! visibility, circular imports, opaque-type discipline, and per-namespace
//! duplicate detection.

use std::fmt::Write;

use hird_ast::{AstNode, SourceFile};
use hird_check::{CheckedProgram, ModuleName, Severity, check_program};

/// Parses each `(module name, source)` pair, checks the program, and renders
/// every module's resolved bindings followed by its diagnostics, in
/// module-name order.
fn check_modules(modules: &[(&str, &str)]) -> String {
    let parsed: Vec<_> = modules
        .iter()
        .map(|(_, src)| hird_parse::parse(src, 0))
        .collect();
    for ((name, src), p) in modules.iter().zip(&parsed) {
        assert!(
            p.is_ok(),
            "module `{name}` has parse errors in `{src}`: {:?}",
            p.diagnostics()
        );
    }
    let files: Vec<(ModuleName, SourceFile)> = modules
        .iter()
        .zip(&parsed)
        .map(|((name, _), p)| {
            (
                ModuleName::new(*name),
                SourceFile::cast(p.syntax().clone()).expect("root is a source file"),
            )
        })
        .collect();
    render(&check_program(&files))
}

/// Parses each `(module name, source)` pair and checks the program, returning
/// the raw result for structural assertions.
fn checked(modules: &[(&str, &str)]) -> CheckedProgram {
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
    check_program(&files)
}

/// Renders a checked program: a header per module, then its bindings and
/// diagnostics (secondary spans indented beneath their diagnostic).
fn render(program: &CheckedProgram) -> String {
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
            for rel in &diag.related {
                writeln!(
                    out,
                    "  related {}..{}: {}",
                    rel.span.start, rel.span.end, rel.message
                )
                .unwrap();
            }
        }
    }
    out
}

// ── import resolution: selective, aliased, wildcard ─────────────

#[test]
fn selective_import_resolves() {
    insta::assert_snapshot!(check_modules(&[
        ("Ets", "module Ets\npub fn lookup(k: Int) -> Int = k"),
        (
            "App",
            "module App\nuse Ets.{lookup}\npub fn run(x: Int) -> Int = lookup(x)",
        ),
    ]));
}

#[test]
fn aliased_import_qualified_call() {
    insta::assert_snapshot!(check_modules(&[
        ("Log", "module Log\npub fn info(msg: String) -> Int = 0"),
        (
            "App",
            "module App\nuse Log as L\npub fn run() -> Int = L.info(\"hi\")",
        ),
    ]));
}

#[test]
fn wildcard_import_qualified_call() {
    insta::assert_snapshot!(check_modules(&[
        ("Ets", "module Ets\npub fn lookup(k: Int) -> Int = k"),
        (
            "App",
            "module App\nuse Ets\npub fn run(x: Int) -> Int = Ets.lookup(x)",
        ),
    ]));
}

#[test]
fn transparent_constructors_importable() {
    insta::assert_snapshot!(check_modules(&[
        ("Opt", "module Opt\npub type Maybe = Just(Int) | Nada"),
        (
            "App",
            "module App\n\
             use Opt.{Maybe, Just, Nada}\n\
             pub fn unwrap(m: Maybe) -> Int = match m { Just(n) -> n, Nada -> 0, }\n\
             pub fn wrap(n: Int) -> Maybe = Just(n)",
        ),
    ]));
}

// ── visibility & qualified-name failures ────────────────────────

#[test]
fn private_name_is_not_exported() {
    insta::assert_snapshot!(check_modules(&[
        ("M", "module M\nfn secret(x: Int) -> Int = x"),
        (
            "App",
            "module App\nuse M.{secret}\npub fn run(x: Int) -> Int = x"
        ),
    ]));
}

#[test]
fn qualified_name_unknown_member() {
    insta::assert_snapshot!(check_modules(&[
        ("Ets", "module Ets\npub fn lookup(k: Int) -> Int = k"),
        (
            "App",
            "module App\nuse Ets\npub fn run(x: Int) -> Int = Ets.nope(x)",
        ),
    ]));
}

#[test]
fn unresolved_module() {
    insta::assert_snapshot!(check_modules(&[(
        "App",
        "module App\nuse Nowhere.{thing}\npub fn run() -> Int = 0",
    )]));
}

// ── circular imports ────────────────────────────────────────────

#[test]
fn circular_import_is_rejected() {
    insta::assert_snapshot!(check_modules(&[
        ("A", "module A\nuse B\npub fn fa() -> Int = 0"),
        ("B", "module B\nuse A\npub fn fb() -> Int = 0"),
    ]));
}

// ── opaque types ────────────────────────────────────────────────

#[test]
fn opaque_capability_used_legitimately() {
    insta::assert_snapshot!(check_modules(&[
        (
            "Ets",
            "module Ets\n\
             pub opaque type Table = MkTable(Int)\n\
             pub fn empty() -> Table = MkTable(0)\n\
             pub fn size(t: Table) -> Int = match t { MkTable(n) -> n, }",
        ),
        (
            "App",
            "module App\n\
             use Ets.{Table, empty, size}\n\
             pub fn use_table() -> Int = size(empty())\n\
             pub fn store(t: Table) -> List<Table> = [t]",
        ),
    ]));
}

#[test]
fn opaque_destructure_outside_module_errors() {
    insta::assert_snapshot!(check_modules(&[
        ("Ets", "module Ets\npub opaque type Table = MkTable(Int)"),
        (
            "App",
            "module App\nuse Ets.{Table}\npub fn peek(t: Table) -> Int = match t { MkTable(n) -> n, }",
        ),
    ]));
}

#[test]
fn opaque_construct_outside_module_errors() {
    insta::assert_snapshot!(check_modules(&[
        ("Ets", "module Ets\npub opaque type Table = MkTable(Int)"),
        (
            "App",
            "module App\nuse Ets.{Table}\npub fn forge() -> Table = MkTable(0)"
        ),
    ]));
}

// ── duplicate / collision detection (two namespaces) ────────────

#[test]
fn duplicate_value_definition() {
    insta::assert_snapshot!(check_modules(&[(
        "M",
        "module M\nfn foo() -> Int = 1\nfn foo() -> Int = 2",
    )]));
}

#[test]
fn duplicate_type_definition() {
    insta::assert_snapshot!(check_modules(&[("M", "module M\ntype T = A\ntype T = B")]));
}

#[test]
fn type_and_constructor_share_a_name() {
    insta::assert_snapshot!(check_modules(&[(
        "M",
        "module M\npub type Email = Email(String)\npub fn make(s: String) -> Email = Email(s)",
    )]));
}

#[test]
fn import_collides_with_local_definition() {
    insta::assert_snapshot!(check_modules(&[
        ("M", "module M\npub fn helper(x: Int) -> Int = x"),
        (
            "App",
            "module App\nuse M.{helper}\nfn helper(x: Int) -> Int = x"
        ),
    ]));
}

#[test]
fn import_collides_with_import() {
    insta::assert_snapshot!(check_modules(&[
        ("A", "module A\npub fn shared() -> Int = 1"),
        ("B", "module B\npub fn shared() -> Int = 2"),
        (
            "App",
            "module App\nuse A.{shared}\nuse B.{shared}\npub fn run() -> Int = shared()",
        ),
    ]));
}

// ── import origins for lowering ─────────────────────────────────

/// Each unshadowed use of an unqualified imported function records its
/// defining module (call and value positions alike); a shadowed use records
/// nothing, and the defining module records nothing for its own functions.
#[test]
fn import_origins_recorded_for_unshadowed_uses() {
    let program = checked(&[
        ("Lib", "module Lib\npub fn double(x: Int) -> Int = x + x"),
        (
            "App",
            "module App\nuse Lib.{double}\n\
             pub fn call(x: Int) -> Int = double(x)\n\
             pub fn value(x: Int) -> Int = let f = double in f(x)\n\
             pub fn shadowed(x: Int) -> Int = let double = \\y -> y in double(x)",
        ),
    ]);
    let app = &program.modules[&ModuleName::new("App")];
    assert!(
        app.diagnostics
            .iter()
            .all(|d| d.severity == Severity::Warning),
        "diags: {:?}",
        app.diagnostics
    );
    let origins: Vec<&str> = app
        .import_origins
        .values()
        .map(ModuleName::as_str)
        .collect();
    assert_eq!(origins, ["Lib", "Lib"], "one origin per unshadowed use");
    let lib = &program.modules[&ModuleName::new("Lib")];
    assert!(
        lib.import_origins.is_empty(),
        "origins: {:?}",
        lib.import_origins
    );
}

// ── module-name validation ──────────────────────────────────────

#[test]
fn module_name_must_match_path() {
    insta::assert_snapshot!(check_modules(&[(
        "Right",
        "module Wrong\npub fn f() -> Int = 0"
    )]));
}

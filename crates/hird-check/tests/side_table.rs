// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Coverage for the per-node type side-table (`CheckedFile::types`).
//!
//! The snapshot suite only observes top-level bindings and diagnostics; it
//! never reads the node table. That table is the deliverable pattern-match
//! exhaustiveness and IR lowering consume, so it is exercised here directly:
//! every visited node must carry a resolved type, polymorphic uses record
//! their per-occurrence instantiation (not the scheme), and pattern nodes
//! are typed by their position in the scrutinee.

use hird_ast::{AstNode, Decl, Expr, FnDecl, Pattern, SourceFile};
use hird_check::{CheckedFile, NodeKey};

/// Parses and checks `source`, returning the projected file alongside the
/// check result. Both own their data, so the tree outlives the borrow.
fn prepare(source: &str) -> (SourceFile, CheckedFile) {
    let parsed = hird_parse::parse(source, 0);
    assert!(
        parsed.is_ok(),
        "test source has parse errors: {:?}",
        parsed.diagnostics()
    );
    let file = SourceFile::cast(parsed.syntax().clone()).expect("root is a source file");
    let checked = hird_check::check(&file, 0);
    assert!(
        !checked.has_errors(),
        "test source has type errors: {:?}",
        checked.diagnostics
    );
    (file, checked)
}

/// The function declaration named `name`.
fn fn_named(file: &SourceFile, name: &str) -> FnDecl {
    file.declarations()
        .find_map(|d| match d {
            Decl::Fn(f) if f.name() == Some(name) => Some(f),
            _ => None,
        })
        .expect("function is present")
}

/// The resolved type recorded for `expr`, rendered display-canonically.
fn expr_ty(checked: &CheckedFile, expr: &Expr) -> String {
    checked
        .type_at(NodeKey::of_expr(expr))
        .unwrap_or_else(|| panic!("no recorded type for {expr:?}"))
        .normalized()
        .to_string()
}

/// The resolved type recorded for `pattern`, rendered display-canonically.
fn pattern_ty(checked: &CheckedFile, pattern: &Pattern) -> String {
    checked
        .type_at(NodeKey::of_node(pattern.syntax()))
        .unwrap_or_else(|| panic!("no recorded type for {pattern:?}"))
        .normalized()
        .to_string()
}

/// A polymorphic `let` binding used at two types records the distinct
/// instantiated type at each call site — information the top-level
/// `bindings` map (which only holds schemes) cannot express.
#[test]
fn polymorphic_use_sites_record_distinct_instantiations() {
    let (file, checked) = prepare(r#"fn main() = let id = \x -> x in (id(1), id("a"))"#);

    let Some(Expr::Let(le)) = fn_named(&file, "main").body() else {
        panic!("main body is a let");
    };
    let Some(Expr::Tuple(tuple)) = le.body() else {
        panic!("let body is a tuple");
    };
    let uses: Vec<Expr> = tuple.elements().collect();
    assert_eq!(uses.len(), 2, "two call sites");

    let callee = |app: &Expr| {
        let Expr::App(app) = app else {
            panic!("element is an application");
        };
        app.function().expect("application has a callee")
    };

    // `id` instantiates to `Int → Int` at the first site and `String →
    // String` at the second; the table holds both, keyed by occurrence.
    let first = expr_ty(&checked, &callee(&uses[0]));
    let second = expr_ty(&checked, &callee(&uses[1]));
    assert_eq!(first, "Int \u{2192} Int");
    assert_eq!(second, "String \u{2192} String");
    assert_ne!(
        first, second,
        "each occurrence is instantiated independently"
    );
}

/// Pattern nodes are recorded and typed by position: the constructor
/// pattern carries the scrutinee type and its sub-pattern carries the
/// field type. This is exactly what exhaustiveness checking reads back.
#[test]
fn pattern_nodes_are_typed_by_position() {
    let (file, checked) = prepare(
        "type Option<a> = Some(a) | None\n\
         fn unwrap(opt: Option<Int>) -> Int = match opt { Some(x) -> x, None -> 0, }",
    );

    let Some(Expr::Match(me)) = fn_named(&file, "unwrap").body() else {
        panic!("unwrap body is a match");
    };
    let first_arm = me.arms().next().expect("match has arms");
    let pattern = first_arm.pattern().expect("first arm has a pattern");

    // The whole `Some(x)` pattern is typed at the scrutinee type.
    assert_eq!(pattern_ty(&checked, &pattern), "Option<Int>");

    // Its single field sub-pattern `x` is typed at the constructor's
    // field type.
    let Pattern::Constructor(ctor) = &pattern else {
        panic!("first arm is a constructor pattern");
    };
    let field = ctor.fields().next().expect("Some has one field pattern");
    assert_eq!(pattern_ty(&checked, &field), "Int");
}

/// Sub-expressions of every visited form land in the table with resolved
/// types — a guard that no inference path forgets to record, and that
/// entries are resolved rather than left as raw variables.
#[test]
fn literals_and_subexpressions_are_recorded() {
    let (file, checked) = prepare(r#"fn main() = (1, "a", True)"#);

    let Some(Expr::Tuple(tuple)) = fn_named(&file, "main").body() else {
        panic!("main body is a tuple");
    };
    let elems: Vec<Expr> = tuple.elements().collect();
    assert_eq!(expr_ty(&checked, &elems[0]), "Int");
    assert_eq!(expr_ty(&checked, &elems[1]), "String");
    assert_eq!(expr_ty(&checked, &elems[2]), "Bool");

    // The tuple expression itself is recorded too.
    let body = fn_named(&file, "main").body().expect("main has a body");
    assert_eq!(expr_ty(&checked, &body), "(Int, String, Bool)");
}

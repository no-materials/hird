// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

use hird_ast::{AstNode, Decl, Expr, FnDecl, SourceFile};

fn file(src: &str) -> SourceFile {
    let parsed = hird_parse::parse(src, 0);
    assert!(parsed.is_ok(), "parse errors: {:?}", parsed.diagnostics());
    SourceFile::cast(parsed.syntax().clone()).expect("source file root")
}

fn first_fn(file: &SourceFile) -> FnDecl {
    file.declarations()
        .find_map(|d| match d {
            Decl::Fn(f) => Some(f),
            _ => None,
        })
        .expect("a fn declaration")
}

/// Parse `fn f() = <expr>` and return the body expression.
fn body(expr_src: &str) -> Expr {
    let src = alloc_fn(expr_src);
    let file = file(&src);
    first_fn(&file).body().expect("a fn body")
}

fn alloc_fn(expr_src: &str) -> String {
    format!("fn f() = {expr_src}")
}

/// Collect an iterator of borrowed strings into owned ones.
fn owned<'a>(it: impl Iterator<Item = &'a str>) -> Vec<String> {
    it.map(str::to_owned).collect()
}

// ── declarations ────────────────────────────────────────────────

#[test]
fn source_file_declarations() {
    let file = file(
        "\
module Planner

use Actors::Base

effect Log

type Result<A> = Ok(A) | Err(String)

pub fn identity(x: Int) -> Int = x

extern fn print(s: String) -> Unit",
    );

    let module_name = file.module().and_then(|m| m.name().map(str::to_owned));
    assert_eq!(module_name.as_deref(), Some("Planner"));

    let kinds: Vec<&str> = file
        .declarations()
        .map(|d| match d {
            Decl::Module(_) => "module",
            Decl::Use(_) => "use",
            Decl::Effect(_) => "effect",
            Decl::Type(_) => "type",
            Decl::Fn(_) => "fn",
            Decl::Extern(_) => "extern",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["module", "use", "effect", "type", "fn", "extern"]);
}

#[test]
fn use_decl_path_and_alias() {
    let file = file("use Foo::Bar::Baz as B");
    let use_decl = file
        .declarations()
        .find_map(|d| match d {
            Decl::Use(u) => Some(u),
            _ => None,
        })
        .unwrap();
    let segments = owned(use_decl.path().expect("path").segments());
    assert_eq!(segments, ["Foo", "Bar", "Baz"]);
    assert_eq!(use_decl.alias(), Some("B"));
}

#[test]
fn fn_decl_basics() {
    let file = file("fn add(x: Int, y: Int) -> Int = x");
    let f = first_fn(&file);
    assert_eq!(f.name(), Some("add"));
    assert!(!f.is_pub());
    // `params()` yields owned nodes, so copy each name before the node drops.
    let params: Vec<String> = f
        .params()
        .filter_map(|p| p.name().map(str::to_owned))
        .collect();
    assert_eq!(params, ["x", "y"]);
    assert!(matches!(f.body(), Some(Expr::Name(_))));
}

#[test]
fn fn_pub_visibility() {
    let file = file("pub fn f() = 0");
    assert!(first_fn(&file).is_pub());
}

#[test]
fn type_decl_constructors() {
    let file = file("type Option<A> = Some(A) | None");
    let ty = file
        .declarations()
        .find_map(|d| match d {
            Decl::Type(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(ty.name(), Some("Option"));
    let ctors: Vec<String> = ty
        .constructors()
        .filter_map(|c| c.name().map(str::to_owned))
        .collect();
    assert_eq!(ctors, ["Some", "None"]);
}

#[test]
fn actor_supervisor_names() {
    let actor_file = file("actor Planner { state: PlannerState }");
    let actor = actor_file
        .declarations()
        .find_map(|d| match d {
            Decl::Actor(a) => Some(a),
            _ => None,
        })
        .unwrap();
    assert_eq!(actor.name(), Some("Planner"));
    assert!(!actor.is_pub());

    let sup_file = file("supervisor PlannerSup { strategy: one_for_one }");
    let sup = sup_file
        .declarations()
        .find_map(|d| match d {
            Decl::Supervisor(s) => Some(s),
            _ => None,
        })
        .unwrap();
    assert_eq!(sup.name(), Some("PlannerSup"));
}

// ── expressions ─────────────────────────────────────────────────

#[test]
fn atomic_bodies_are_tokens() {
    // The parser does not wrap atoms in nodes, so a bare literal/ident body is
    // projected as a token-backed Expr variant rather than being lost.
    let Expr::Literal(lit) = body("42") else {
        panic!("expected literal");
    };
    assert_eq!(lit.text(), "42");

    let Expr::Name(name) = body("x") else {
        panic!("expected name");
    };
    assert_eq!(name.text(), "x");
}

#[test]
fn let_expr_parts() {
    let Expr::Let(e) = body("let x = 42 in x") else {
        panic!("expected let");
    };
    assert_eq!(e.name(), Some("x"));
    assert!(matches!(e.value(), Some(Expr::Literal(_))));
    assert!(matches!(e.body(), Some(Expr::Name(_))));
}

#[test]
fn lambda_expr_parts() {
    let Expr::Lambda(e) = body("\\x y -> x") else {
        panic!("expected lambda");
    };
    assert_eq!(owned(e.param_names()), ["x", "y"]);
    assert!(matches!(e.body(), Some(Expr::Name(_))));
}

#[test]
fn if_expr_parts() {
    let Expr::If(e) = body("if c then a else b") else {
        panic!("expected if");
    };
    assert!(matches!(e.condition(), Some(Expr::Name(_))));
    assert!(matches!(e.then_branch(), Some(Expr::Name(_))));
    assert!(matches!(e.else_branch(), Some(Expr::Name(_))));
}

#[test]
fn match_expr_parts() {
    let Expr::Match(e) = body("match x { Foo -> 1, Bar -> 2 }") else {
        panic!("expected match");
    };
    assert!(matches!(e.scrutinee(), Some(Expr::Name(_))));
    let arm_bodies: Vec<String> = e
        .arms()
        .filter_map(|a| match a.body() {
            Some(Expr::Literal(l)) => Some(l.text().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(arm_bodies, ["1", "2"]);
}

#[test]
fn handle_block_parts() {
    let Expr::Handle(e) = body("handle { Log -> a, Tool<ReadRepo> -> b } in m") else {
        panic!("expected handle");
    };
    assert_eq!(e.arms().count(), 2);
    assert!(matches!(e.body(), Some(Expr::Name(_))));
    let first = e.arms().next().unwrap();
    assert!(matches!(first.handler(), Some(Expr::Name(_))));
}

#[test]
fn bin_op_precedence() {
    // `x + y * z` parses as `x + (y * z)`.
    let Expr::BinOp(add) = body("x + y * z") else {
        panic!("expected binop");
    };
    assert_eq!(add.op(), Some("+"));
    assert!(matches!(add.lhs(), Some(Expr::Name(_))));
    let Some(Expr::BinOp(mul)) = add.rhs() else {
        panic!("expected nested binop on the right");
    };
    assert_eq!(mul.op(), Some("*"));
}

#[test]
fn app_expr_is_left_nested() {
    // `f x y` parses as `(f x) y`.
    let Expr::App(outer) = body("f x y") else {
        panic!("expected application");
    };
    assert!(matches!(outer.function(), Some(Expr::App(_))));
    assert!(matches!(outer.argument(), Some(Expr::Name(_))));
}

#[test]
fn field_expr_parts() {
    // `a.b.c` parses as `(a.b).c`.
    let Expr::Field(outer) = body("a.b.c") else {
        panic!("expected field access");
    };
    assert_eq!(outer.field(), Some("c"));
    assert!(matches!(outer.receiver(), Some(Expr::Field(_))));
}

#[test]
fn record_lit_fields_and_keyword_key() {
    let Expr::Record(rec) = body("{ actor: Planner, count: 5 }") else {
        panic!("expected record");
    };
    let names: Vec<String> = rec
        .fields()
        .filter_map(|f| f.name().map(str::to_owned))
        .collect();
    assert_eq!(names, ["actor", "count"]);
    let first = rec.fields().next().unwrap();
    assert!(matches!(first.value(), Some(Expr::Name(_))));
}

#[test]
fn tuple_list_paren() {
    let Expr::Tuple(t) = body("(a, b, c)") else {
        panic!("expected tuple");
    };
    assert_eq!(t.elements().count(), 3);

    let Expr::List(l) = body("[1, 2]") else {
        panic!("expected list");
    };
    assert_eq!(l.elements().count(), 2);

    let Expr::Paren(p) = body("(x)") else {
        panic!("expected paren");
    };
    assert!(matches!(p.inner(), Some(Expr::Name(_))));
}

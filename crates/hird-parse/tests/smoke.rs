// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

use std::fmt::Write;

use hird_parse::SyntaxKind;

fn render_cst(source: &str) -> String {
    let result = hird_parse::parse(source, 0);
    let root = cstree::syntax::SyntaxNode::<SyntaxKind>::new_root(result.green().clone());
    let mut out = String::new();
    render_node(&root, source, &mut out, 0);
    if !result.is_ok() {
        writeln!(out, "---").unwrap();
        for d in result.diagnostics() {
            writeln!(
                out,
                "{} {}..{}: {}",
                d.code.as_str(),
                d.span.start,
                d.span.end,
                d.message
            )
            .unwrap();
        }
    }
    out
}

fn render_node(
    node: &cstree::syntax::SyntaxNode<SyntaxKind>,
    source: &str,
    out: &mut String,
    indent: usize,
) {
    let pad = "  ".repeat(indent);
    writeln!(out, "{pad}{:?}", node.kind()).unwrap();
    for child in node.children_with_tokens() {
        match child {
            cstree::util::NodeOrToken::Node(n) => render_node(n, source, out, indent + 1),
            cstree::util::NodeOrToken::Token(t) => {
                let range = t.text_range();
                let start = usize::from(range.start());
                let end = usize::from(range.end());
                let text = &source[start..end];
                let pad2 = "  ".repeat(indent + 1);
                writeln!(out, "{pad2}{:?} {:?}", t.kind(), text).unwrap();
            }
        }
    }
}

// ── basics ──────────────────────────────────────────────────────

#[test]
fn empty_source() {
    insta::assert_snapshot!(render_cst(""));
}

#[test]
fn whitespace_only() {
    insta::assert_snapshot!(render_cst("  \n  "));
}

// ── module declaration ──────────────────────────────────────────

#[test]
fn module_decl() {
    insta::assert_snapshot!(render_cst("module Foo"));
}

// ── use declarations ────────────────────────────────────────────

#[test]
fn use_simple() {
    insta::assert_snapshot!(render_cst("use Foo"));
}

#[test]
fn use_path() {
    insta::assert_snapshot!(render_cst("use Foo::Bar::Baz"));
}

#[test]
fn use_alias() {
    insta::assert_snapshot!(render_cst("use Foo::Bar as B"));
}

// ── function declarations ───────────────────────────────────────

#[test]
fn fn_minimal() {
    insta::assert_snapshot!(render_cst("fn foo() = 42"));
}

#[test]
fn fn_with_params() {
    insta::assert_snapshot!(render_cst("fn add(x: Int, y: Int) = x"));
}

#[test]
fn fn_with_return_type() {
    insta::assert_snapshot!(render_cst("fn id(x: Int) -> Int = x"));
}

#[test]
fn fn_with_effect() {
    insta::assert_snapshot!(render_cst("fn log(msg: String) -> Unit ! {Log} = msg"));
}

#[test]
fn fn_pub() {
    insta::assert_snapshot!(render_cst("pub fn foo() = 42"));
}

#[test]
fn fn_trailing_comma_params() {
    insta::assert_snapshot!(render_cst("fn f(x: Int, y: Int,) = x"));
}

// ── error recovery ──────────────────────────────────────────────

#[test]
fn invalid_type_not_misread_as_effect() {
    insta::assert_snapshot!(render_cst("fn f(x: !) = x"));
}

// ── type declarations ───────────────────────────────────────────

#[test]
fn type_simple_adt() {
    insta::assert_snapshot!(render_cst("type Bool = True | False"));
}

#[test]
fn type_with_params_and_fields() {
    insta::assert_snapshot!(render_cst("type Option<A> = Some(A) | None"));
}

#[test]
fn type_multi_field_constructor() {
    insta::assert_snapshot!(render_cst("type Pair<A, B> = Pair(A, B)"));
}

// ── actor declarations ──────────────────────────────────────────

#[test]
fn actor_decl() {
    insta::assert_snapshot!(render_cst("actor MyActor { state: Int, init: create }"));
}

// ── supervisor declarations ─────────────────────────────────────

#[test]
fn supervisor_decl() {
    insta::assert_snapshot!(render_cst(
        "supervisor MySup { strategy: one_for_one, intensity: 5 }"
    ));
}

// ── effect declarations ─────────────────────────────────────────

#[test]
fn effect_simple() {
    insta::assert_snapshot!(render_cst("effect Log"));
}

#[test]
fn effect_with_params() {
    insta::assert_snapshot!(render_cst("effect State<S>"));
}

// ── tool declarations ───────────────────────────────────────────

#[test]
fn tool_decl() {
    insta::assert_snapshot!(render_cst("tool Fetch : Url -> Response"));
}

// ── extern declarations ─────────────────────────────────────────

#[test]
fn extern_decl() {
    insta::assert_snapshot!(render_cst("extern fn print(s: String) -> Unit"));
}

// ── expressions ─────────────────────────────────────────────────

#[test]
fn expr_let() {
    insta::assert_snapshot!(render_cst("fn f() = let x = 42 in x"));
}

#[test]
fn expr_let_with_type_ann() {
    insta::assert_snapshot!(render_cst("fn f() = let x : Int = 42 in x"));
}

#[test]
fn expr_lambda() {
    insta::assert_snapshot!(render_cst("fn f() = \\x -> x"));
}

#[test]
fn expr_lambda_multi_param() {
    insta::assert_snapshot!(render_cst("fn f() = \\x y -> x"));
}

#[test]
fn expr_if() {
    insta::assert_snapshot!(render_cst("fn f() = if true then 1 else 0"));
}

#[test]
fn expr_paren() {
    insta::assert_snapshot!(render_cst("fn f() = (42)"));
}

// ── type expressions ────────────────────────────────────────────

#[test]
fn type_fn_type_return() {
    insta::assert_snapshot!(render_cst("fn f() -> Int -> Bool = x"));
}

#[test]
fn type_applied() {
    insta::assert_snapshot!(render_cst("fn f(x: List<Int>) = x"));
}

#[test]
fn type_tuple() {
    insta::assert_snapshot!(render_cst("fn f(x: (Int, Bool)) = x"));
}

// ── comments preserved ──────────────────────────────────────────

#[test]
fn comment_before_decl() {
    insta::assert_snapshot!(render_cst("// a function\nfn foo() = 42"));
}

// ── multi-declaration module ────────────────────────────────────

#[test]
fn multi_decl_module() {
    insta::assert_snapshot!(render_cst(
        "\
module Planner

use Actors::Base

effect Log

type Result<A> = Ok(A) | Err(String)

pub fn identity(x: Int) -> Int = x

extern fn print(s: String) -> Unit"
    ));
}

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
    insta::assert_snapshot!(render_cst("use Foo.Bar.Baz"));
}

#[test]
fn use_alias() {
    insta::assert_snapshot!(render_cst("use Foo.Bar as B"));
}

#[test]
fn use_selective() {
    insta::assert_snapshot!(render_cst("use Ets.{Table, lookup}"));
}

#[test]
fn use_selective_trailing_comma() {
    insta::assert_snapshot!(render_cst("use Ets.{Table, lookup,}"));
}

#[test]
fn use_selective_empty_is_error() {
    // A selective group must name at least one member.
    insta::assert_snapshot!(render_cst("use Ets.{}"));
}

#[test]
fn use_selective_leading_comma_is_error() {
    // A leading separator has no name before it.
    insta::assert_snapshot!(render_cst("use Ets.{,}"));
}

#[test]
fn use_selective_with_alias_is_error() {
    // Selective and aliased forms are mutually exclusive.
    insta::assert_snapshot!(render_cst("use Ets.{Table} as E"));
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

#[test]
fn deep_nesting_produces_diagnostic() {
    let depth = 300;
    let mut src = String::from("fn f() = ");
    for _ in 0..depth {
        src.push_str("if ");
    }
    src.push('x');
    for _ in 0..depth {
        src.push_str(" then x else x");
    }
    let result = hird_parse::parse(&src, 0);
    assert!(
        !result.is_ok(),
        "deeply nested input should produce a diagnostic"
    );
    assert!(
        result
            .diagnostics()
            .iter()
            .any(|d| d.message == "nesting depth limit reached"),
        "expected nesting-depth diagnostic, got: {:?}",
        result.diagnostics()
    );
}

#[test]
fn deep_application_nesting_produces_diagnostic() {
    // Covers the operator/application recursion path; the test above covers
    // prefix recursion. Both must terminate with a diagnostic, not hang.
    let depth = 1000;
    let mut src = String::from("fn f() = ");
    for _ in 0..depth {
        src.push_str("f (");
    }
    src.push('x');
    for _ in 0..depth {
        src.push(')');
    }
    let result = hird_parse::parse(&src, 0);
    assert!(
        !result.is_ok(),
        "deeply nested application should produce a diagnostic"
    );
    assert!(
        result
            .diagnostics()
            .iter()
            .any(|d| d.message == "nesting depth limit reached"),
        "expected nesting-depth diagnostic, got: {:?}",
        result.diagnostics()
    );
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

#[test]
fn type_leading_pipe() {
    insta::assert_snapshot!(render_cst("type PlannerMsg = | PlanRepo(Path) | Shutdown"));
}

// ── type visibility: private / pub / pub opaque ─────────────────

#[test]
fn type_private() {
    insta::assert_snapshot!(render_cst("type Foo = Bar(Int)"));
}

#[test]
fn type_pub() {
    insta::assert_snapshot!(render_cst("pub type Foo = Bar(Int)"));
}

#[test]
fn type_pub_opaque() {
    insta::assert_snapshot!(render_cst("pub opaque type Foo = Bar(Int)"));
}

#[test]
fn type_opaque_without_pub_is_error() {
    // `opaque` must follow `pub`. The bare form is reported, but `opaque` alone
    // is wrapped in an error and the type still recovers as a private one.
    insta::assert_snapshot!(render_cst("opaque type Foo = Bar(Int)"));
}

#[test]
fn type_opaque_without_type_is_error() {
    // `opaque` only modifies a `type`. Here it precedes `fn`: the `pub opaque`
    // run is wrapped in an error and the function still parses.
    insta::assert_snapshot!(render_cst("pub opaque fn foo() = 1"));
}

// ── actor declarations ──────────────────────────────────────────

#[test]
fn actor_decl() {
    insta::assert_snapshot!(render_cst("actor MyActor { state: Int, init: create }"));
}

#[test]
fn actor_message_leading_pipe() {
    insta::assert_snapshot!(render_cst("actor A { message: M = | Foo | Bar }"));
}

#[test]
fn actor_init_signature() {
    insta::assert_snapshot!(render_cst(
        "actor A { init: fn(c: Config) -> St ! {Log} = create(c) }"
    ));
}

#[test]
fn actor_handler_clause() {
    insta::assert_snapshot!(render_cst(
        "actor A { handle Msg(x), st -> St ! {Log} = f(x, st) }"
    ));
}

#[test]
fn actor_handler_missing_state_pattern() {
    // The state pattern is required; its absence is reported but the rest of
    // the handler still parses.
    insta::assert_snapshot!(render_cst("actor A { handle Msg(x) -> St = f(x) }"));
}

#[test]
fn actor_full() {
    insta::assert_snapshot!(render_cst(
        "\
actor Planner {
  state: PlannerState,
  message: PlannerMsg =
    | PlanRepo(Path)
    | GetStatus(ReplyTo<PlannerStatus>)
    | Shutdown,
  init: fn(config: PlannerConfig) -> PlannerState ! {Log} = initial_state(config),
  handle PlanRepo(path), st -> PlannerState ! {Tool<ReadRepo>, Log} = update(path, st),
  handle Shutdown, st -> PlannerState ! {} = st,
} ! {Tool<ReadRepo>, Log}"
    ));
}

// ── spawn expressions ───────────────────────────────────────────

#[test]
fn spawn_expr() {
    insta::assert_snapshot!(render_cst("fn go(c: Config) = spawn(Planner, c)"));
}

#[test]
fn spawn_no_args() {
    insta::assert_snapshot!(render_cst("fn go() = spawn(Worker)"));
}

// ── messaging expressions ───────────────────────────────────────

#[test]
fn send_expr() {
    insta::assert_snapshot!(render_cst("fn go(p: Pid<Msg>) = send(p, Stop)"));
}

#[test]
fn request_expr() {
    insta::assert_snapshot!(render_cst("fn go(p: Pid<Msg>) = request(p, GetStatus)"));
}

#[test]
fn reply_expr() {
    insta::assert_snapshot!(render_cst("fn go(r: ReplyTo<Int>) = reply(r, 42)"));
}

#[test]
fn send_missing_message_recovers() {
    insta::assert_snapshot!(render_cst("fn go(p: Pid<Msg>) = send(p)"));
}

// ── crash expressions ───────────────────────────────────────────

#[test]
fn crash_expr() {
    insta::assert_snapshot!(render_cst(r#"fn go() = crash!("boom")"#));
}

#[test]
fn panic_expr() {
    insta::assert_snapshot!(render_cst(r#"fn go() = panic!("boom")"#));
}

#[test]
fn crash_missing_bang_recovers() {
    // The `!` is required; without it the parser flags the missing marker.
    insta::assert_snapshot!(render_cst(r#"fn go() = crash("boom")"#));
}

// ── supervisor declarations ─────────────────────────────────────

#[test]
fn supervisor_decl() {
    insta::assert_snapshot!(render_cst(
        "supervisor MySup { strategy: one_for_one, intensity: 5 }"
    ));
}

#[test]
fn supervisor_full() {
    insta::assert_snapshot!(render_cst(
        "supervisor PlannerSup { strategy: one_for_one, intensity: 5, period: 60, \
         children: [ { id: planner, actor: Planner, start_args: default_config(), \
         restart: permanent } ] }"
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

#[test]
fn tool_decl_record_input() {
    insta::assert_snapshot!(render_cst("tool ReadRepo : { path: Path } -> RepoState"));
}

#[test]
fn tool_decl_generic_with_trailing_row() {
    insta::assert_snapshot!(render_cst(
        "tool LLMCall<t> : { prompt: Prompt, schema: Schema<t> } -> t ! {Exn<ParseError>}"
    ));
}

// ── record types ────────────────────────────────────────────────

#[test]
fn record_type_in_param() {
    insta::assert_snapshot!(render_cst("fn get(r: { x: Int, y: Int }) -> Int = 1"));
}

#[test]
fn record_type_empty() {
    insta::assert_snapshot!(render_cst("fn take(r: {}) -> Int = 1"));
}

#[test]
fn record_type_keyword_field_name() {
    insta::assert_snapshot!(render_cst("fn take(r: { tool: String }) -> Int = 1"));
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

#[test]
fn expr_binary_precedence() {
    insta::assert_snapshot!(render_cst("fn f() = x + y * z"));
}

#[test]
fn expr_binary_paren_precedence() {
    insta::assert_snapshot!(render_cst("fn f() = (x + y) * z"));
}

#[test]
fn expr_relational_chain_rejected() {
    // Relational operators are one non-associative tier; chaining is a P0005.
    insta::assert_snapshot!(render_cst("fn f() = a == b == c"));
}

#[test]
fn expr_relational_mixed_chain_rejected() {
    // Mixing operators within the tier is still a chain: `<` then `==`.
    insta::assert_snapshot!(render_cst("fn f() = a < b == c"));
}

#[test]
fn expr_relational_parenthesised_ok() {
    // Parenthesising the left operand makes the comparison well-formed.
    insta::assert_snapshot!(render_cst("fn f() = (a == b) == c"));
}

#[test]
fn expr_arithmetic_then_comparison_ok() {
    // A single comparison over an arithmetic operand is not a chain: the
    // left-associative `+` resets the non-associative tracking.
    insta::assert_snapshot!(render_cst("fn f() = a + b == c"));
}

#[test]
fn expr_logical_and() {
    insta::assert_snapshot!(render_cst("fn f() = a && b"));
}

#[test]
fn expr_logical_or() {
    insta::assert_snapshot!(render_cst("fn f() = a || b"));
}

#[test]
fn expr_logical_precedence() {
    // `&&` binds tighter than `||`: `a || b && c` is `a || (b && c)`.
    insta::assert_snapshot!(render_cst("fn f() = a || b && c"));
}

#[test]
fn expr_logical_looser_than_relational() {
    // Relational binds tighter than logical, and a comparison on each side of
    // `&&` is well-formed (no chain): `(a == b) && (c < d)`.
    insta::assert_snapshot!(render_cst("fn f() = a == b && c < d"));
}

#[test]
fn expr_application_chain() {
    insta::assert_snapshot!(render_cst("fn f() = f x y"));
}

#[test]
fn expr_application_binds_tighter_than_binary() {
    insta::assert_snapshot!(render_cst("fn f() = f x + g y"));
}

#[test]
fn expr_field_chain() {
    insta::assert_snapshot!(render_cst("fn f() = x.y.z"));
}

#[test]
fn expr_application_with_field_arg() {
    insta::assert_snapshot!(render_cst("fn f() = f x.y"));
}

// ── literals ────────────────────────────────────────────────────

#[test]
fn expr_tuple() {
    insta::assert_snapshot!(render_cst("fn f() = (a, b)"));
}

#[test]
fn expr_tuple_trailing_comma() {
    insta::assert_snapshot!(render_cst("fn f() = (a, b,)"));
}

#[test]
fn expr_unit() {
    insta::assert_snapshot!(render_cst("fn f() = ()"));
}

#[test]
fn expr_list() {
    insta::assert_snapshot!(render_cst("fn f() = [a, b, c]"));
}

#[test]
fn expr_list_empty() {
    insta::assert_snapshot!(render_cst("fn f() = []"));
}

#[test]
fn expr_record() {
    insta::assert_snapshot!(render_cst("fn f() = { x: 1, y: 2 }"));
}

#[test]
fn expr_record_empty() {
    insta::assert_snapshot!(render_cst("fn f() = {}"));
}

#[test]
fn expr_record_keyword_key() {
    // Keyword spellings (`actor`, `type`) are valid record field names.
    insta::assert_snapshot!(render_cst("fn f() = { actor: Planner, type: Foo }"));
}

#[test]
fn expr_list_of_records() {
    insta::assert_snapshot!(render_cst("fn f() = [{ x: 1 }, { x: 2 }]"));
}

#[test]
fn expr_record_of_lists() {
    insta::assert_snapshot!(render_cst("fn f() = { xs: [1, 2], ys: [3] }"));
}

#[test]
fn expr_application_list_arg() {
    insta::assert_snapshot!(render_cst("fn f() = g [1, 2]"));
}

// ── match expressions ───────────────────────────────────────────

#[test]
fn expr_match_basic() {
    insta::assert_snapshot!(render_cst("fn f() = match x { Foo -> 1, Bar -> 2 }"));
}

#[test]
fn expr_match_constructor_args() {
    insta::assert_snapshot!(render_cst(
        "fn f() = match msg { PlanRepo(path) -> path, Shutdown -> x }"
    ));
}

#[test]
fn expr_match_ctor_vs_binding() {
    // `None` is PascalCase -> CONSTRUCTOR_PAT; `y` is snake_case -> BIND_PAT.
    insta::assert_snapshot!(render_cst("fn f() = match x { None -> 0, y -> y }"));
}

#[test]
fn expr_match_wildcard() {
    insta::assert_snapshot!(render_cst("fn f() = match x { _ -> 0 }"));
}

#[test]
fn expr_match_literal() {
    insta::assert_snapshot!(render_cst("fn f() = match n { 0 -> a, 1 -> b }"));
}

#[test]
fn expr_match_tuple_pattern() {
    insta::assert_snapshot!(render_cst("fn f() = match p { (a, b) -> a }"));
}

#[test]
fn expr_match_nested_pattern() {
    insta::assert_snapshot!(render_cst("fn f() = match x { Foo(Bar(y), _) -> y }"));
}

#[test]
fn expr_match_trailing_comma() {
    insta::assert_snapshot!(render_cst("fn f() = match x { A -> 1, }"));
}

#[test]
fn deep_pattern_nesting_produces_diagnostic() {
    // Patterns are a new recursion site; the depth guard must terminate them
    // with a diagnostic rather than overflow the stack.
    let depth = 1000;
    let mut src = String::from("fn f() = match x { ");
    for _ in 0..depth {
        src.push_str("Foo(");
    }
    src.push('y');
    for _ in 0..depth {
        src.push(')');
    }
    src.push_str(" -> y }");
    let result = hird_parse::parse(&src, 0);
    assert!(
        !result.is_ok(),
        "deeply nested pattern should produce a diagnostic"
    );
    assert!(
        result
            .diagnostics()
            .iter()
            .any(|d| d.message == "nesting depth limit reached"),
        "expected nesting-depth diagnostic, got: {:?}",
        result.diagnostics()
    );
}

// ── handle expressions ──────────────────────────────────────────

#[test]
fn expr_handle_basic() {
    insta::assert_snapshot!(render_cst("fn f() = handle { Log -> capturing } in body"));
}

#[test]
fn expr_handle_parametric_effect() {
    insta::assert_snapshot!(render_cst(
        "fn f() = handle { Tool<ReadRepo> -> mock_read } in run(config)"
    ));
}

#[test]
fn expr_handle_multi_arm() {
    insta::assert_snapshot!(render_cst(
        "fn f() = handle { Tool<ReadRepo> -> a, Log -> b } in planner_main(config)"
    ));
}

#[test]
fn expr_handle_trailing_comma() {
    insta::assert_snapshot!(render_cst("fn f() = handle { Log -> a, } in m"));
}

// ── install expressions ─────────────────────────────────────────

#[test]
fn expr_install_basic() {
    insta::assert_snapshot!(render_cst(
        "fn f() = install { Tool<ReadRepo> -> demo_read } in run_demo(config)"
    ));
}

#[test]
fn expr_install_multi_arm() {
    insta::assert_snapshot!(render_cst(
        "fn f() = install { Tool<ReadRepo> -> a, Log -> b, } in m"
    ));
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
fn type_paren() {
    insta::assert_snapshot!(render_cst("fn f(x: (Int)) = x"));
}

#[test]
fn type_unit() {
    insta::assert_snapshot!(render_cst("fn f(x: ()) = x"));
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

use Actors.Base

effect Log

type Result<A> = Ok(A) | Err(String)

pub fn identity(x: Int) -> Int = x

extern fn print(s: String) -> Unit"
    ));
}

// ── error recovery: the five recovery patterns ──────────────────
//
// Each snapshot shows the recovered CST followed by the diagnostics
// (code, span, message). Recovery never drops the rest of the input:
// where a trailing declaration is present it still parses.

#[test]
fn recover_missing_closing_delimiter() {
    // The `(` is never closed. The parser reports the missing `)` and finishes
    // the `PAREN_EXPR` in place (a synthetic close — no bytes are invented), so
    // the following declaration still parses.
    insta::assert_snapshot!(render_cst("fn f() = (a + b\nfn g() = 3"));
}

#[test]
fn recover_unexpected_token_mid_expression() {
    // `*` cannot start an operand. Recovery skips the stray run (`* b`) into an
    // `ERROR` node up to the synchronisation point `)`, which then closes the
    // parenthesised expression.
    insta::assert_snapshot!(render_cst("fn f() = (a + * b)"));
}

#[test]
fn recover_incomplete_declaration() {
    // The function name is missing. The parser records the error, parses what
    // remains of the declaration, and continues to the next one.
    insta::assert_snapshot!(render_cst("fn () = 1\nfn g() = 2"));
}

#[test]
fn recover_malformed_type_annotation() {
    // A literal is not a type. The annotation falls back to an `ERROR` node and
    // the rest of the declaration (`)`, `=`, body) still parses.
    insta::assert_snapshot!(render_cst("fn f(x: 1) = x"));
}

#[test]
fn recover_missing_eq_before_fn_body() {
    // `fn foo() 42` omits the `=` before the body. The parser reports it and
    // parses `42` as the body anyway.
    insta::assert_snapshot!(render_cst("fn foo() 42"));
}

#[test]
fn recover_unexpected_token_in_declaration() {
    // Stray tokens where a declaration is expected are skipped as a run into a
    // single `ERROR` node up to the next declaration keyword; the surrounding
    // declarations are untouched.
    insta::assert_snapshot!(render_cst("type T = A\n99 88\nfn g() = 2"));
}

#[test]
fn recovery_terminates_and_stays_lossless() {
    // Pathological inputs must terminate (no panic, no hang — the harness
    // `timeout` guards the latter) and the CST must still reproduce the
    // original bytes exactly, error nodes included.
    let inputs = [
        "",
        "   ",
        "}",
        "]",
        ")",
        ",",
        ") ) )",
        "} } }",
        "fn",
        "fn f(",
        "fn f()",
        "fn f() =",
        "fn f() = (",
        "fn f() = (a + * b",
        "fn f() = [1,",
        "fn f() = { x:",
        "fn f() = )",
        "fn f() = ) ] } fn g() = 1",
        "fn f() = if",
        "fn f() = match x {",
        "fn f() = handle {",
        "fn f(x: ) = x",
        "fn f(x: <) = x",
        "fn f(x: List<) = x",
        "pub",
        "pub pub",
        "pub 42",
        "type",
        "type T =",
        "type T = |",
        "effect",
        "effect E<",
        "use",
        "42 99",
        "+ + +",
        "99 88 fn g() = 1",
    ];
    for src in inputs {
        let parsed = hird_parse::parse(src, 0);
        assert!(
            parsed.syntax().text() == src,
            "CST is not lossless for {src:?}"
        );
    }
}

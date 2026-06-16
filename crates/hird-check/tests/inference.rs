// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

use std::fmt::Write;

use hird_ast::{AstNode, SourceFile};
use hird_check::Severity;

/// Parses, checks, and renders `source` as resolved top-level bindings
/// followed by diagnostics.
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

// ── let-polymorphism ────────────────────────────────────────────

#[test]
fn let_polymorphism_identity() {
    insta::assert_snapshot!(check_str(r"fn main() = let id = \x -> x in id(1)"));
}

#[test]
fn polymorphic_let_used_at_two_types() {
    insta::assert_snapshot!(check_str(
        r#"fn main() = let f = \x -> x in (f(1), f("hello"))"#
    ));
}

#[test]
fn monomorphic_lambda_rejected() {
    insta::assert_snapshot!(check_str(r#"fn main() = \f -> (f(1), f("hello"))"#));
}

#[test]
fn unannotated_function_generalizes() {
    insta::assert_snapshot!(check_str(r"fn pair(x: a, y: b) = (x, y)"));
}

#[test]
fn empty_list_generalizes() {
    insta::assert_snapshot!(check_str(r"fn main() = []"));
}

// ── ADT declarations and constructors ───────────────────────────

#[test]
fn adt_constructors_are_typed() {
    insta::assert_snapshot!(check_str(
        "type Option<a> = Some(a) | None\n\
         fn some_one() = Some(1)\n\
         fn none_val() = None"
    ));
}

#[test]
fn recursive_adt() {
    insta::assert_snapshot!(check_str(
        "type List<a> = Cons(a, List<a>) | Nil\n\
         fn main() = Cons(1, Cons(2, Nil))"
    ));
}

#[test]
fn duplicate_type_parameter() {
    insta::assert_snapshot!(check_str("type Pair<a, a> = P(a)"));
}

#[test]
fn unbound_type_parameter() {
    insta::assert_snapshot!(check_str("type Box<a> = B(b)"));
}

// ── conditionals ────────────────────────────────────────────────

#[test]
fn if_branches_unify() {
    insta::assert_snapshot!(check_str(r"fn main() = if True then 1 else 2"));
}

#[test]
fn if_branch_mismatch() {
    insta::assert_snapshot!(check_str(r#"fn main() = if True then 1 else "two""#));
}

#[test]
fn if_condition_must_be_bool() {
    insta::assert_snapshot!(check_str(r"fn main() = if 1 then 2 else 3"));
}

// ── annotations and signatures ──────────────────────────────────

#[test]
fn annotated_fn_signature() {
    insta::assert_snapshot!(check_str(r"fn add(x: Int, y: Int) -> Int = x + y"));
}

#[test]
fn annotated_polymorphic_identity() {
    insta::assert_snapshot!(check_str(
        "fn id(x: a) -> a = x\n\
         fn main() = (id(1), id(\"hi\"))"
    ));
}

#[test]
fn signature_more_general_than_body() {
    insta::assert_snapshot!(check_str(r"fn id(x: a) -> a = 1"));
}

#[test]
fn let_annotation() {
    insta::assert_snapshot!(check_str(r"fn main() = let x: Int = 1 in x"));
}

#[test]
fn let_annotation_mismatch() {
    insta::assert_snapshot!(check_str(r"fn main() = let x: String = 1 in x"));
}

#[test]
fn let_annotation_polymorphic() {
    insta::assert_snapshot!(check_str(
        r#"fn main() = let f: a -> a = \x -> x in (f(1), f("hi"))"#
    ));
}

#[test]
fn higher_order_return_type() {
    insta::assert_snapshot!(check_str(
        "fn make_adder(n: Int) -> (Int -> Int) = \\m -> n + m\n\
         fn main() = make_adder(1)(2)"
    ));
}

// ── recursion and ordering ──────────────────────────────────────

#[test]
fn recursive_function() {
    insta::assert_snapshot!(check_str(
        r"fn fact(n: Int) -> Int = if n <= 1 then 1 else n * fact(n - 1)"
    ));
}

#[test]
fn mutual_recursion_inferred() {
    insta::assert_snapshot!(check_str(
        "fn is_even(n: Int) = if n == 0 then True else is_odd(n - 1)\n\
         fn is_odd(n: Int) = if n == 0 then False else is_even(n - 1)"
    ));
}

#[test]
fn forward_reference() {
    insta::assert_snapshot!(check_str(
        "fn main() = helper(1)\n\
         fn helper(x: a) = x"
    ));
}

// ── application shape ───────────────────────────────────────────

#[test]
fn call_arity_mismatch() {
    insta::assert_snapshot!(check_str(
        "fn add(x: Int, y: Int) -> Int = x + y\n\
         fn main() = add(1)"
    ));
}

#[test]
fn tuple_value_vs_argument_list() {
    insta::assert_snapshot!(check_str(
        "fn first(p: (Int, String)) -> Int = match p { (x, y) -> x, }\n\
         fn good() = first((1, \"a\"))\n\
         fn bad() = first(1, \"a\")"
    ));
}

#[test]
fn unit_function() {
    insta::assert_snapshot!(check_str(
        "fn unit() = ()\n\
         fn main() = unit()"
    ));
}

#[test]
fn infinite_type_occurs_check() {
    insta::assert_snapshot!(check_str(r"fn omega(f: a) = f(f)"));
}

// ── pattern matching ────────────────────────────────────────────

#[test]
fn constructor_pattern_match() {
    insta::assert_snapshot!(check_str(
        "type Option<a> = Some(a) | None\n\
         fn unwrap_or(opt: Option<Int>, default: Int) -> Int = match opt {\n\
           Some(x) -> x,\n\
           None -> default,\n\
         }"
    ));
}

#[test]
fn nested_patterns() {
    insta::assert_snapshot!(check_str(
        "type List<a> = Cons(a, List<a>) | Nil\n\
         type Option<a> = Some(a) | None\n\
         fn first_or_zero(opt: Option<List<Int>>) -> Int = match opt {\n\
           Some(Cons(x, _)) -> x,\n\
           Some(Nil) -> 0,\n\
           None -> 0,\n\
         }"
    ));
}

#[test]
fn match_arms_must_unify() {
    insta::assert_snapshot!(check_str(
        "fn main() = match 1 {\n\
           1 -> 1,\n\
           _ -> \"many\",\n\
         }"
    ));
}

#[test]
fn constructor_pattern_arity() {
    insta::assert_snapshot!(check_str(
        "type Option<a> = Some(a) | None\n\
         fn main() = match Some(1) {\n\
           Some(x, y) -> x,\n\
           None -> 0,\n\
         }"
    ));
}

#[test]
fn unknown_constructor_in_pattern() {
    insta::assert_snapshot!(check_str(
        "fn main() = match 1 {\n\
           Whatever(x) -> x,\n\
         }"
    ));
}

#[test]
fn tuple_pattern_swap() {
    insta::assert_snapshot!(check_str(
        "fn swap(p: (Int, String)) -> (String, Int) = match p { (x, y) -> (y, x), }"
    ));
}

#[test]
fn literal_pattern() {
    insta::assert_snapshot!(check_str(
        "fn describe(n: Int) -> String = match n {\n\
           1 -> \"one\",\n\
           _ -> \"many\",\n\
         }"
    ));
}

#[test]
fn bool_is_an_adt() {
    insta::assert_snapshot!(check_str(
        "fn flip(b: Bool) -> Bool = match b {\n\
           True -> False,\n\
           False -> True,\n\
         }"
    ));
}

// ── exhaustiveness & redundancy ──────────────────────────────────

#[test]
fn non_exhaustive_missing_constructor() {
    insta::assert_snapshot!(check_str(
        "type Option<a> = Some(a) | None\n\
         fn unwrap(opt: Option<Int>) -> Int = match opt {\n\
           Some(x) -> x,\n\
         }"
    ));
}

#[test]
fn non_exhaustive_lists_all_missing() {
    insta::assert_snapshot!(check_str(
        "type Color = Red | Green | Blue\n\
         fn code(c: Color) -> Int = match c {\n\
           Red -> 0,\n\
         }"
    ));
}

#[test]
fn non_exhaustive_bool() {
    insta::assert_snapshot!(check_str(
        "fn f(b: Bool) -> Int = match b {\n\
           True -> 1,\n\
         }"
    ));
}

#[test]
fn wildcard_arm_is_exhaustive() {
    insta::assert_snapshot!(check_str(
        "type Option<a> = Some(a) | None\n\
         fn unwrap_or(opt: Option<Int>, d: Int) -> Int = match opt {\n\
           Some(x) -> x,\n\
           _ -> d,\n\
         }"
    ));
}

#[test]
fn variable_arm_is_exhaustive() {
    insta::assert_snapshot!(check_str(
        "type Option<a> = Some(a) | None\n\
         fn or_zero(opt: Option<Int>) -> Int = match opt {\n\
           Some(x) -> x,\n\
           rest -> 0,\n\
         }"
    ));
}

#[test]
fn literals_need_a_wildcard() {
    insta::assert_snapshot!(check_str(
        "fn describe(n: Int) -> String = match n {\n\
           1 -> \"one\",\n\
           2 -> \"two\",\n\
         }"
    ));
}

#[test]
fn redundant_duplicate_literal() {
    insta::assert_snapshot!(check_str(
        "fn describe(n: Int) -> String = match n {\n\
           1 -> \"one\",\n\
           1 -> \"uno\",\n\
           _ -> \"many\",\n\
         }"
    ));
}

#[test]
fn redundant_arm_after_wildcard() {
    insta::assert_snapshot!(check_str(
        "type Option<a> = Some(a) | None\n\
         fn f(opt: Option<Int>) -> Int = match opt {\n\
           _ -> 0,\n\
           Some(x) -> x,\n\
         }"
    ));
}

#[test]
fn redundant_constructor_arm() {
    insta::assert_snapshot!(check_str(
        "type Option<a> = Some(a) | None\n\
         fn f(opt: Option<Int>) -> Int = match opt {\n\
           Some(x) -> x,\n\
           None -> 0,\n\
           Some(y) -> y,\n\
         }"
    ));
}

#[test]
fn nested_non_exhaustive() {
    insta::assert_snapshot!(check_str(
        "type List<a> = Cons(a, List<a>) | Nil\n\
         type Option<a> = Some(a) | None\n\
         fn first(opt: Option<List<Int>>) -> Int = match opt {\n\
           Some(Cons(x, _)) -> x,\n\
           None -> 0,\n\
         }"
    ));
}

#[test]
fn tuple_missing_combination() {
    insta::assert_snapshot!(check_str(
        "fn f(p: (Bool, Bool)) -> Int = match p {\n\
           (True, True) -> 1,\n\
           (False, False) -> 0,\n\
         }"
    ));
}

#[test]
fn tuple_components_exhaustive() {
    insta::assert_snapshot!(check_str(
        "fn f(p: (Bool, Bool)) -> Int = match p {\n\
           (True, _) -> 1,\n\
           (False, _) -> 0,\n\
         }"
    ));
}

#[test]
fn empty_match_is_non_exhaustive() {
    insta::assert_snapshot!(check_str(
        "type Option<a> = Some(a) | None\n\
         fn f(opt: Option<Int>) -> Int = match opt { }"
    ));
}

// ── operators ───────────────────────────────────────────────────

#[test]
fn operator_table() {
    insta::assert_snapshot!(check_str(
        r#"fn main() = (1 + 2 * 3, 1 < 2, "a" == "a", True && False)"#
    ));
}

#[test]
fn operator_mismatch() {
    insta::assert_snapshot!(check_str(r#"fn main() = 1 + "one""#));
}

// ── records ─────────────────────────────────────────────────────

#[test]
fn record_field_access() {
    insta::assert_snapshot!(check_str(r"fn main() = let r = { age: 1 } in r.age"));
}

#[test]
fn field_access_needs_known_record() {
    insta::assert_snapshot!(check_str(r"fn get(r: a) = r.age"));
}

#[test]
fn missing_record_field() {
    insta::assert_snapshot!(check_str(r"fn main() = let r = { age: 1 } in r.name"));
}

// ── scoping ─────────────────────────────────────────────────────

#[test]
fn shadowing_warns() {
    insta::assert_snapshot!(check_str(r"fn main() = let x = 1 in let x = 2 in x"));
}

#[test]
fn unbound_name() {
    insta::assert_snapshot!(check_str(r"fn main() = nope"));
}

// ── type annotations gone wrong ─────────────────────────────────

#[test]
fn unknown_type_name() {
    insta::assert_snapshot!(check_str(r"fn f(x: Nope) -> Int = 1"));
}

#[test]
fn type_argument_arity() {
    insta::assert_snapshot!(check_str(r"fn f(x: Option) -> Int = 1"));
}

// ── externs ─────────────────────────────────────────────────────

#[test]
fn extern_signature() {
    insta::assert_snapshot!(check_str(
        "extern fn list_len(xs: List<a>) -> Int\n\
         fn main() = list_len([])"
    ));
}

#[test]
fn extern_requires_full_signature() {
    insta::assert_snapshot!(check_str("extern fn now()"));
}

// ── let-polymorphism soundness ──────────────────────────────────

/// A `let` value that is just a renamed lambda parameter must NOT be
/// generalised: `y` is tied to the monomorphic `x`, so using it at two
/// types is an error. This is the core soundness property of HM
/// let-generalisation — every positive let-poly test still passes if the
/// level discipline regresses, so only this negative case guards it.
#[test]
fn captured_monomorphic_binding_is_not_generalized() {
    insta::assert_snapshot!(check_str(
        r#"fn main() = \x -> let y = x in (y(1), y("a"))"#
    ));
}

/// Two distinct signature variables are rigid and distinct: a body that
/// returns the second where the first is demanded must fail, even though
/// both are "just type variables".
#[test]
fn distinct_skolems_do_not_unify() {
    insta::assert_snapshot!(check_str(r"fn const_first(x: a, y: b) -> a = y"));
}

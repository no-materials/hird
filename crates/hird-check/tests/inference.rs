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

// ── effect rows ─────────────────────────────────────────────────

/// An explicit empty row is the pure row and elides in display.
#[test]
fn empty_effect_row_elides() {
    insta::assert_snapshot!(check_str("fn add(x: Int, y: Int) -> Int ! {} = x + y"));
}

/// A single declared effect appears on the scheme, and the body — applying an
/// effectful parameter — performs exactly it.
#[test]
fn single_effect_on_signature() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         fn log_it(run: Int -> Int ! {Log}) -> Int ! {Log} = run(0)"
    ));
}

/// Several effects share the row, rendered head-sorted.
#[test]
fn multiple_effects_on_signature() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         effect Spawn\n\
         fn worker(run: Int -> Int ! {Spawn, Log}) -> Int ! {Spawn, Log} = run(0)"
    ));
}

/// A parametric effect carries a type argument (`Tool<Repo>`).
#[test]
fn parametric_effect_on_signature() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         type Repo = MkRepo\n\
         fn read(run: Int -> Int ! {Tool<Repo>}) -> Int ! {Tool<Repo>} = run(0)"
    ));
}

/// A row-polymorphic function: the row variable `r` is shared between the
/// callback's effect row and the function's own, and quantified in the scheme.
#[test]
fn row_polymorphic_signature() {
    insta::assert_snapshot!(check_str(
        r"fn apply(g: a -> b ! {r}, x: a) -> b ! {r} = g(x)"
    ));
}

/// An effect annotation referencing an undeclared effect is an error.
#[test]
fn unknown_effect_is_rejected() {
    insta::assert_snapshot!(check_str("fn f(x: Int) -> Int ! {Mystery} = x"));
}

/// Applying an effect to the wrong number of type arguments is an error.
#[test]
fn effect_arity_mismatch_is_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         fn f(x: Int) -> Int ! {Tool} = x"
    ));
}

/// A row may name at most one row variable.
#[test]
fn multiple_row_variables_rejected() {
    insta::assert_snapshot!(check_str(r"fn f(x: Int) -> Int ! {r, s} = x"));
}

// ── effect inference and annotation checking ─────────────────────

/// Applying a pure function adds no effects, so a body of pure calls needs no
/// effect annotation.
#[test]
fn pure_application_adds_no_effect() {
    insta::assert_snapshot!(check_str(
        r"fn apply_pure(g: Int -> Int, x: Int) -> Int = g(x)"
    ));
}

/// Sequential `let` bindings union their effects: the value's and the body's.
#[test]
fn sequential_lets_union_effects() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         effect Spawn\n\
         fn f(a: Int -> Int ! {Log}, b: Int -> Int ! {Spawn}) -> Int ! {Log, Spawn} =\n\
           let x = a(0) in b(x)"
    ));
}

/// A match unions the scrutinee's effects with the join of its arms'.
#[test]
fn match_unions_scrutinee_and_arms() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         effect Spawn\n\
         fn f(s: Int -> Bool ! {Log}, a: Int -> Int ! {Spawn}) -> Int ! {Log, Spawn} =\n\
           match s(0) { True -> a(0), False -> 0, }"
    ));
}

/// An `if` unions the condition's effects with both branches'.
#[test]
fn if_unions_branch_effects() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         effect Spawn\n\
         fn f(c: Int -> Bool ! {Log}, t: Int -> Int ! {Spawn}) -> Int ! {Log, Spawn} =\n\
           if c(0) then t(0) else 0"
    ));
}

/// A lambda's body effects belong to its function type, not the enclosing
/// function: `make` is pure, but the lambda it returns carries `{Log}`.
#[test]
fn lambda_row_on_function_type() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         fn make(run: Int -> Int ! {Log}) = \\n -> run(n)"
    ));
}

/// A lambda that is defined but never applied contributes no effects to its
/// enclosing function, so the pure annotation holds.
#[test]
fn unused_lambda_defers_effects() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         fn f(run: Int -> Int ! {Log}) -> Int = let h = \\n -> run(n) in 0"
    ));
}

/// A nested (let-bound) function's effects reach the enclosing row only where it
/// is applied.
#[test]
fn nested_function_effect_on_call() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         fn f(run: Int -> Int ! {Log}) -> Int ! {Log} = let h = \\n -> run(n) in h(0)"
    ));
}

/// An interior function infers a row-polymorphic type with no annotation: the
/// callback's effects flow through to the result's row.
#[test]
fn inferred_row_polymorphism() {
    insta::assert_snapshot!(check_str(r"fn main() = let apply = \g x -> g(x) in apply"));
}

/// A declared row may mix a concrete effect with a row variable; the body's
/// inferred row unifies against it.
#[test]
fn concrete_and_polymorphic_row() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         fn logged(g: a -> b ! {Log, r}, x: a) -> b ! {Log, r} = g(x)"
    ));
}

/// A capability effect carries the capability parameter's type: `EtsRead<t>`
/// elaborates to `EtsRead<Table<…>>`.
#[test]
fn capability_effect_carries_type() {
    insta::assert_snapshot!(check_str(
        "type Table<k, v, p> = MkTable\n\
         effect EtsRead<t>\n\
         fn query(\n\
           t: Table<Int, String, Bool>,\n\
           run: Table<Int, String, Bool> -> Int ! {EtsRead<t>}\n\
         ) -> Int ! {EtsRead<t>} = run(t)"
    ));
}

/// Two differently-typed capabilities give two distinct effects in the row.
#[test]
fn distinct_capability_types_stay_distinct() {
    insta::assert_snapshot!(check_str(
        "type Table<k, v, p> = MkTable\n\
         effect EtsRead<t>\n\
         fn query(\n\
           t1: Table<Int, String, Bool>,\n\
           t2: Table<Bool, Int, String>,\n\
           r1: Table<Int, String, Bool> -> Int ! {EtsRead<t1>},\n\
           r2: Table<Bool, Int, String> -> Int ! {EtsRead<t2>}\n\
         ) -> Int ! {EtsRead<t1>, EtsRead<t2>} = let a = r1(t1) in r2(t2)"
    ));
}

/// Two same-typed capabilities collapse to one row element — the documented
/// v0.1 limitation: the row distinguishes resource *type*, not binding.
#[test]
fn same_typed_capabilities_collapse() {
    insta::assert_snapshot!(check_str(
        "type Table<k, v, p> = MkTable\n\
         effect EtsRead<t>\n\
         fn query(\n\
           t1: Table<Int, String, Bool>,\n\
           t2: Table<Int, String, Bool>,\n\
           r1: Table<Int, String, Bool> -> Int ! {EtsRead<t1>},\n\
           r2: Table<Int, String, Bool> -> Int ! {EtsRead<t2>}\n\
         ) -> Int ! {EtsRead<t1>} = let a = r1(t1) in r2(t2)"
    ));
}

/// An effectful body under a pure (absent) annotation is rejected, pointing at
/// the call that introduced the effect.
#[test]
fn under_declared_effect_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         fn f(run: Int -> Int ! {Log}) -> Int = run(0)"
    ));
}

/// A mismatch names both rows and points at the call introducing the offending
/// effect (`tooler(0)`, the `Tool<X>` call), not the whole signature.
#[test]
fn mismatch_points_at_offending_call() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         effect Tool<t>\n\
         type X = MkX\n\
         fn f(logger: Int -> Int ! {Log}, tooler: Int -> Int ! {Tool<X>}) -> Int ! {Log} =\n\
           let a = logger(0) in tooler(0)"
    ));
}

/// Equality, not subsumption: a declared effect the body never performs is also
/// rejected, keeping the row honest.
#[test]
fn over_declared_effect_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         effect Spawn\n\
         fn f(run: Int -> Int ! {Log}) -> Int ! {Log, Spawn} = run(0)"
    ));
}

// ── DI-style effect handlers ─────────────────────────────────────
//
// A `handle` block's row is the body's effects minus the handled effects plus
// the handlers' own effects, so the enclosing function declares only what
// escapes the block.

/// Handling the effect a body performs removes it from the block's row, so the
/// enclosing function may declare itself pure.
#[test]
fn handle_subtracts_handled_effect() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         fn handled(run: Int -> Int ! {Log}, h: Int -> Int) -> Int = handle { Log -> h } in run(0)"
    ));
}

/// An effect the body performs but no arm handles stays in the block's row.
#[test]
fn handle_leaves_unhandled_effect() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         effect Tool<t>\n\
         type Repo = MkRepo\n\
         fn partial(f: Int -> Int ! {Log, Tool<Repo>}, h: Int -> Int) -> Int ! {Tool<Repo>} =\n\
           handle { Log -> h } in f(0)"
    ));
}

/// A handler's own effects join the block's row: handling `Tool<Repo>` with a
/// logging handler trades the tool effect for `Log`.
#[test]
fn handle_adds_handler_effects() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         effect Tool<t>\n\
         tool Repo : { x: Int } -> Int\n\
         fn audited(f: Int -> Int ! {Tool<Repo>}, logh: { x: Int } -> Int ! {Log}) -> Int ! {Log} =\n\
           handle { Tool<Repo> -> logh } in f(0)"
    ));
}

/// Several arms handle several effects at once; handling them all clears the
/// row.
#[test]
fn handle_multiple_arms() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         effect Tool<t>\n\
         tool Repo : { x: Int } -> Int\n\
         fn multi(f: Int -> Int ! {Log, Tool<Repo>}, lh: Int -> Int, th: { x: Int } -> Int) -> Int =\n\
           handle { Log -> lh, Tool<Repo> -> th } in f(0)"
    ));
}

/// Nested handles each subtract one effect; the inner block's row becomes the
/// outer block's body.
#[test]
fn handle_nested_blocks() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         effect Tool<t>\n\
         tool Repo : { x: Int } -> Int\n\
         fn nested(f: Int -> Int ! {Log, Tool<Repo>}, lh: Int -> Int, th: { x: Int } -> Int) -> Int =\n\
           handle { Log -> lh } in handle { Tool<Repo> -> th } in f(0)"
    ));
}

/// An arm whose head is not a declared effect is rejected (unknown effect).
#[test]
fn handle_unknown_effect_rejected() {
    insta::assert_snapshot!(check_str(
        "fn bad(h: Int -> Int) -> Int = handle { Bogus -> h } in 0"
    ));
}

/// An arm head applied at the wrong arity is rejected (`Tool` takes one
/// argument).
#[test]
fn handle_effect_arity_mismatch_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         fn bad(h: Int -> Int) -> Int = handle { Tool -> h } in 0"
    ));
}

/// A handler that is not a function is rejected: a bare-label effect has no
/// operation signature, so the handler check is structural.
#[test]
fn handle_non_function_handler_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Log\n\
         fn bad() -> Int = handle { Log -> 42 } in 0"
    ));
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Lowering coverage: distinct typed programs lowered to IR, with the
//! structure and the JSON projection checked directly.

use hird_ast::{AstNode, SourceFile};
use hird_ir::{IrDecl, IrExpr, IrModule, IrPattern, LiteralValue, lower_module};

/// Parses, checks, and lowers `source` into a module named `name`. Panics on
/// any parse or type error so a malformed test surfaces immediately.
fn lower(source: &str, name: &str) -> IrModule {
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
    lower_module(&file, &checked, name)
}

/// The single function definition of a one-function module.
fn only_fn(module: &IrModule) -> &hird_ir::IrFnDef {
    let [IrDecl::Fn(f)] = module.declarations.as_slice() else {
        panic!(
            "expected exactly one function, got {:?}",
            module.declarations
        );
    };
    f
}

/// Renders a type display-canonically.
fn ty_str(ty: &hird_types::Type) -> String {
    format!("{ty}")
}

// ── operators desugar to application ─────────────────────────────

#[test]
fn binary_operator_lowers_to_application() {
    let module = lower("fn add(x: Int, y: Int) -> Int = x + y", "Math");
    let add = only_fn(&module);

    assert_eq!(add.name, "add");
    assert_eq!(add.params.len(), 2);
    assert_eq!(add.params[0].name, "x");
    assert_eq!(ty_str(&add.params[0].ty), "Int");
    assert_eq!(ty_str(&add.return_type), "Int");
    // Empty effect row for now.
    assert_eq!(add.effect_row, hird_ir::EffectRow::empty());

    // `x + y` becomes application of the `+` primitive.
    let IrExpr::App(app) = &add.body else {
        panic!(
            "operator body should lower to application, got {:?}",
            add.body
        );
    };
    let IrExpr::Var(op) = app.func.as_ref() else {
        panic!("callee should be the operator reference");
    };
    assert_eq!(op.name, "+");
    assert_eq!(ty_str(&op.ty), "Int \u{2192} Int \u{2192} Int");
    assert_eq!(app.args.len(), 2);
    assert_eq!(ty_str(&app.result_type), "Int");
    let (IrExpr::Var(lhs), IrExpr::Var(rhs)) = (&app.args[0], &app.args[1]) else {
        panic!("operands should be variable references");
    };
    assert_eq!(lhs.name, "x");
    assert_eq!(rhs.name, "y");
}

#[test]
fn logical_operator_normalises_to_unicode() {
    // Written ASCII; the operator reference is the canonical Unicode form.
    let module = lower("fn both(p: Bool, q: Bool) -> Bool = p && q", "Logic");
    let IrExpr::App(app) = &only_fn(&module).body else {
        panic!("expected application");
    };
    let IrExpr::Var(op) = app.func.as_ref() else {
        panic!("expected operator reference");
    };
    assert_eq!(op.name, "\u{2227}", "`&&` canonicalises to `\u{2227}`");
}

// ── let, lambda, application ─────────────────────────────────────

#[test]
fn let_lambda_and_application() {
    let module = lower(r"fn main() = let id = \x -> x in id(1)", "Main");
    let main = only_fn(&module);
    assert!(main.params.is_empty());

    let IrExpr::Let(le) = &main.body else {
        panic!("body should be a let, got {:?}", main.body);
    };
    assert_eq!(le.name, "id");

    // The bound value is the identity lambda; its body reuses the parameter,
    // so the two share a type.
    let IrExpr::Lambda(lambda) = le.value.as_ref() else {
        panic!("let value should be a lambda");
    };
    assert_eq!(lambda.params.len(), 1);
    assert_eq!(lambda.params[0].name, "x");
    let IrExpr::Var(body_var) = lambda.body.as_ref() else {
        panic!("lambda body should be a variable");
    };
    assert_eq!(body_var.name, "x");
    assert_eq!(
        body_var.ty, lambda.params[0].ty,
        "the body variable has the parameter's type"
    );
    assert_eq!(body_var.ty, lambda.body_type);

    // The let body applies `id` to `1`, instantiated at `Int → Int`.
    let IrExpr::App(app) = le.body.as_ref() else {
        panic!("let body should be an application");
    };
    let IrExpr::Var(callee) = app.func.as_ref() else {
        panic!("callee should be a variable");
    };
    assert_eq!(callee.name, "id");
    assert_eq!(ty_str(&callee.ty), "Int \u{2192} Int");
    assert_eq!(ty_str(&app.result_type), "Int");
    let [IrExpr::Literal(one)] = app.args.as_slice() else {
        panic!("one integer argument");
    };
    assert_eq!(one.value, LiteralValue::Int("1".into()));
    assert_eq!(ty_str(&one.ty), "Int");
}

// ── ADTs, constructors, and match ────────────────────────────────

#[test]
fn adt_constructors_and_match() {
    let module = lower(
        "type Option<a> = Some(a) | None\n\
         fn unwrap(opt: Option<Int>) -> Int = match opt { Some(x) -> x, None -> 0, }",
        "Opt",
    );

    // The type definition: one parameter, two constructors, the field of
    // `Some` rendered with the declared parameter name.
    let IrDecl::Type(def) = &module.declarations[0] else {
        panic!("first declaration is the type");
    };
    assert_eq!(def.name, "Option");
    assert_eq!(def.params, ["a"]);
    assert_eq!(def.constructors.len(), 2);
    assert_eq!(def.constructors[0].name, "Some");
    assert_eq!(ty_str(&def.constructors[0].fields[0]), "a");
    assert_eq!(def.constructors[1].name, "None");
    assert!(def.constructors[1].fields.is_empty());

    let IrDecl::Fn(unwrap) = &module.declarations[1] else {
        panic!("second declaration is the function");
    };
    let IrExpr::Match(me) = &unwrap.body else {
        panic!("body should be a match");
    };
    assert_eq!(ty_str(&me.scrutinee_type), "Option<Int>");
    assert_eq!(ty_str(&me.result_type), "Int");
    assert_eq!(me.arms.len(), 2);

    // First arm: `Some(x) -> x`.
    let IrPattern::Constructor(some_pat) = &me.arms[0].pattern else {
        panic!("first arm matches a constructor");
    };
    assert_eq!(some_pat.name, "Some");
    assert_eq!(some_pat.type_name, "Option");
    assert_eq!(ty_str(&some_pat.ty), "Option<Int>");
    let [IrPattern::Bind(bind)] = some_pat.fields.as_slice() else {
        panic!("`Some` binds one field");
    };
    assert_eq!(bind.name, "x");
    assert_eq!(ty_str(&bind.ty), "Int");

    // Second arm: `None -> 0`; the nullary constructor knows its owner.
    let IrPattern::Constructor(none_pat) = &me.arms[1].pattern else {
        panic!("second arm matches a constructor");
    };
    assert_eq!(none_pat.name, "None");
    assert_eq!(none_pat.type_name, "Option");
    assert!(none_pat.fields.is_empty());
    let IrExpr::Literal(zero) = &me.arms[1].body else {
        panic!("`None` arm body is the literal 0");
    };
    assert_eq!(zero.value, LiteralValue::Int("0".into()));
}

#[test]
fn nullary_constructor_reference_knows_its_type() {
    let module = lower(
        "type Option<a> = Some(a) | None\n\
         fn nothing() -> Option<Int> = None",
        "Opt",
    );
    let IrDecl::Fn(f) = &module.declarations[1] else {
        panic!("second declaration is the function");
    };
    let IrExpr::Constructor(ctor) = &f.body else {
        panic!("body is the `None` constructor, got {:?}", f.body);
    };
    assert_eq!(ctor.name, "None");
    assert_eq!(ctor.type_name, "Option");
    assert!(ctor.args.is_empty());
    assert_eq!(ty_str(&ctor.result_type), "Option<Int>");
}

#[test]
fn recursive_adt_field_types_use_parameter_names() {
    let module = lower(
        "type List<a> = Cons(a, List<a>) | Nil\n\
         fn build() = Cons(1, Cons(2, Nil))",
        "Lst",
    );

    let IrDecl::Type(def) = &module.declarations[0] else {
        panic!("first declaration is the type");
    };
    assert_eq!(def.params, ["a"]);
    let cons = &def.constructors[0];
    assert_eq!(cons.name, "Cons");
    assert_eq!(ty_str(&cons.fields[0]), "a");
    assert_eq!(ty_str(&cons.fields[1]), "List<a>");

    // The body is a nested constructor application.
    let IrDecl::Fn(build) = &module.declarations[1] else {
        panic!("second declaration is the function");
    };
    let IrExpr::Constructor(outer) = &build.body else {
        panic!("body is a `Cons` application");
    };
    assert_eq!(outer.name, "Cons");
    assert_eq!(outer.type_name, "List");
    assert_eq!(outer.args.len(), 2);
    assert_eq!(ty_str(&outer.result_type), "List<Int>");
    let IrExpr::Constructor(inner) = &outer.args[1] else {
        panic!("second argument is the nested `Cons`");
    };
    assert_eq!(inner.name, "Cons");
}

// ── if desugars to match over Bool ───────────────────────────────

#[test]
fn if_desugars_to_match_over_bool() {
    let module = lower("fn pick(b: Bool) -> Int = if b then 1 else 2", "Cond");
    let IrExpr::Match(me) = &only_fn(&module).body else {
        panic!(
            "`if` should desugar to a match, got {:?}",
            only_fn(&module).body
        );
    };

    assert_eq!(ty_str(&me.scrutinee_type), "Bool");
    assert_eq!(ty_str(&me.result_type), "Int");

    // Two synthetic arms: `True -> 1`, `False -> 2`.
    let names: Vec<&str> = me
        .arms
        .iter()
        .map(|arm| match &arm.pattern {
            IrPattern::Constructor(c) => c.name.as_str(),
            other => panic!("if-arm should be a constructor pattern, got {other:?}"),
        })
        .collect();
    assert_eq!(names, ["True", "False"]);

    let IrExpr::Var(scrutinee) = me.scrutinee.as_ref() else {
        panic!("scrutinee is the condition variable");
    };
    assert_eq!(scrutinee.name, "b");
    let IrExpr::Literal(then_lit) = &me.arms[0].body else {
        panic!("then-branch is the literal 1");
    };
    assert_eq!(then_lit.value, LiteralValue::Int("1".into()));
}

// ── tuples, lists, unit, and literals ────────────────────────────

#[test]
fn tuple_list_unit_and_literals() {
    let module = lower(
        "fn triple() = (1, \"a\", True)\n\
         fn nums() = [1, 2, 3]\n\
         fn nothing() = ()",
        "Lits",
    );

    let IrDecl::Fn(triple) = &module.declarations[0] else {
        panic!("triple");
    };
    let IrExpr::Tuple(tuple) = &triple.body else {
        panic!("body is a tuple");
    };
    assert_eq!(tuple.elems.len(), 3);
    assert_eq!(ty_str(&tuple.ty), "(Int, String, Bool)");
    let IrExpr::Literal(s) = &tuple.elems[1] else {
        panic!("second element is a string literal");
    };
    assert_eq!(s.value, LiteralValue::Str("\"a\"".into()));
    let IrExpr::Constructor(t) = &tuple.elems[2] else {
        panic!("third element is the True constructor");
    };
    assert_eq!(t.name, "True");
    assert_eq!(t.type_name, "Bool");

    let IrDecl::Fn(nums) = &module.declarations[1] else {
        panic!("nums");
    };
    let IrExpr::List(list) = &nums.body else {
        panic!("body is a list");
    };
    assert_eq!(list.elems.len(), 3);
    assert_eq!(ty_str(&list.ty), "List<Int>");

    let IrDecl::Fn(nothing) = &module.declarations[2] else {
        panic!("nothing");
    };
    let IrExpr::Tuple(unit) = &nothing.body else {
        panic!("body is unit (empty tuple)");
    };
    assert!(unit.elems.is_empty());
    assert_eq!(ty_str(&unit.ty), "()");
}

// ── externs ──────────────────────────────────────────────────────

#[test]
fn extern_reference_carries_its_type() {
    let module = lower("extern fn sqrt(x: Float) -> Float", "Ffi");
    let [IrDecl::Extern(ext)] = module.declarations.as_slice() else {
        panic!("expected one extern, got {:?}", module.declarations);
    };
    assert_eq!(ext.name, "sqrt");
    assert_eq!(ty_str(&ext.ty), "Float \u{2192} Float");
    assert_eq!(ext.module, None);
}

// ── JSON serialization ───────────────────────────────────────────

#[test]
fn json_schema_is_stable() {
    // A tiny program pins the core schema exactly.
    let module = lower("fn answer() = 42", "Main");
    let json = module.to_json().expect("serialization succeeds");
    assert_eq!(
        json,
        r#"{"name":"Main","declarations":[{"kind":"Fn","name":"answer","params":[],"return_type":"Int","effect_row":{},"body":{"kind":"Literal","value":{"Int":"42"},"type":"Int"}}]}"#
    );
}

#[test]
fn json_pretty_snapshot() {
    let module = lower(
        "type Option<a> = Some(a) | None\n\
         fn unwrap(opt: Option<Int>) -> Int = match opt { Some(x) -> x, None -> 0, }",
        "Opt",
    );
    insta::assert_snapshot!(module.to_json_pretty().expect("serialization succeeds"));
}

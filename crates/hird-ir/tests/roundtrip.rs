// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The round-trip property: lowering is stable through pretty-printing.
//!
//! For a well-typed module, `source → check → lower → pretty_print → check →
//! lower` reproduces the first IR up to type-variable renaming. The property
//! catches pretty-printer bugs (output that fails to parse or re-check),
//! lowering bugs (information lost on the way down), and inference instability
//! (re-checking the printed form producing different types).
//!
//! Equality is taken modulo type-variable renaming: inference assigns fresh
//! variable identities on each run, and the printer may turn an inferred
//! signature into a skolemised one, so genuine unification variables and
//! skolem constants are both renumbered by first appearance before comparing.
//! Type declarations are compared verbatim — their constructor fields are
//! fixed by the declared parameter names, with no inference freedom.

use hird_ast::{AstNode, SourceFile};
use hird_ir::{
    IrApp, IrArm, IrBindPat, IrConstructor, IrConstructorPat, IrDecl, IrExpr, IrExternRef, IrField,
    IrFnDef, IrLambda, IrLet, IrList, IrLiteral, IrLiteralPat, IrMatch, IrModule, IrParam,
    IrPattern, IrRecord, IrRecordField, IrTuple, IrTuplePat, IrVar, IrWildcardPat, lower_module,
    pretty_print,
};
use hird_types::Type;
use proptest::prelude::*;
use std::collections::BTreeMap;

// ── harness ──────────────────────────────────────────────────────

/// Parses, checks, and lowers `source`, panicking on any parse or type error
/// (a malformed program must surface immediately, not produce partial IR).
fn lower_src(source: &str, name: &str) -> IrModule {
    let parsed = hird_parse::parse(source, 0);
    assert!(
        parsed.is_ok(),
        "source failed to parse: {source}\n{:?}",
        parsed.diagnostics()
    );
    let file = SourceFile::cast(parsed.syntax().clone()).expect("root is a source file");
    let checked = hird_check::check(&file, 0);
    assert!(
        !checked.has_errors(),
        "source failed to type-check: {source}\n{:?}",
        checked.diagnostics
    );
    lower_module(&file, &checked, name)
}

/// Asserts the round-trip property for `source`: lowering it, pretty-printing,
/// and re-lowering the printed form yields a structurally equal IR (modulo
/// type-variable renaming).
fn assert_roundtrips(source: &str) {
    let first = lower_src(source, "M");
    let printed = pretty_print(&first);
    let second = lower_src(&printed, "M");
    assert_eq!(
        normalize(&first),
        normalize(&second),
        "round-trip changed the IR\n--- source ---\n{source}\n--- printed ---\n{printed}\n\
         --- first ---\n{first:#?}\n--- second ---\n{second:#?}"
    );
}

// ── type-variable normalisation ──────────────────────────────────
//
// A copy of the printer's canonicalisation, applied across a whole function or
// extern so two alpha-equivalent IRs become byte-for-byte equal.

/// Identity of a type variable: a unification index or a skolem name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum VarKey {
    /// A unification variable, by index.
    Unif(u32),
    /// A skolem constant, by name.
    Skolem(String),
}

/// Maps each type variable to its canonical index within one declaration.
type VarMap = BTreeMap<VarKey, u32>;

/// The canonical index for `key`, allocating the next on first sight.
fn intern(map: &mut VarMap, key: VarKey) -> u32 {
    let next = u32::try_from(map.len()).unwrap_or(u32::MAX);
    *map.entry(key).or_insert(next)
}

/// Whether a type name is a variable (lowercase) rather than a constructor.
fn is_type_var(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_lowercase)
}

/// `ty` with every variable (unification variable or skolem) renumbered by
/// first appearance through `map`.
fn canon_type(ty: &Type, map: &mut VarMap) -> Type {
    match ty {
        Type::TyVar(id) => Type::TyVar(intern(map, VarKey::Unif(*id))),
        Type::TyCon(name, args) if args.is_empty() && is_type_var(name.as_str()) => {
            Type::TyVar(intern(map, VarKey::Skolem(String::from(name.as_str()))))
        }
        Type::TyCon(name, args) => Type::TyCon(
            name.clone(),
            args.iter().map(|a| canon_type(a, map)).collect(),
        ),
        Type::TyFn(params, ret) => Type::TyFn(
            params.iter().map(|p| canon_type(p, map)).collect(),
            Box::new(canon_type(ret, map)),
        ),
        Type::TyTuple(elems) => Type::TyTuple(elems.iter().map(|e| canon_type(e, map)).collect()),
        Type::TyRecord(fields) => Type::TyRecord(
            fields
                .iter()
                .map(|(label, v)| (label.clone(), canon_type(v, map)))
                .collect(),
        ),
        Type::TyForall(vars, body) => Type::TyForall(
            vars.iter().map(|v| intern(map, VarKey::Unif(*v))).collect(),
            Box::new(canon_type(body, map)),
        ),
    }
}

/// A copy of `module` with each function's and extern's type variables
/// renumbered. Type declarations are left untouched.
fn normalize(module: &IrModule) -> IrModule {
    IrModule {
        name: module.name.clone(),
        declarations: module.declarations.iter().map(normalize_decl).collect(),
    }
}

/// Normalises one declaration's type variables (with a per-declaration map),
/// or returns a type declaration verbatim.
fn normalize_decl(decl: &IrDecl) -> IrDecl {
    match decl {
        IrDecl::Fn(f) => {
            let mut map = VarMap::new();
            IrDecl::Fn(IrFnDef {
                name: f.name.clone(),
                params: f.params.iter().map(|p| canon_param(p, &mut map)).collect(),
                return_type: canon_type(&f.return_type, &mut map),
                effect_row: f.effect_row,
                body: canon_expr(&f.body, &mut map),
            })
        }
        IrDecl::Type(t) => IrDecl::Type(t.clone()),
        IrDecl::Extern(e) => {
            let mut map = VarMap::new();
            IrDecl::Extern(IrExternRef {
                name: e.name.clone(),
                ty: canon_type(&e.ty, &mut map),
                module: e.module.clone(),
            })
        }
    }
}

/// Normalises a parameter's type.
fn canon_param(param: &IrParam, map: &mut VarMap) -> IrParam {
    IrParam {
        name: param.name.clone(),
        ty: canon_type(&param.ty, map),
    }
}

/// Normalises every type in an expression tree.
fn canon_expr(expr: &IrExpr, map: &mut VarMap) -> IrExpr {
    match expr {
        IrExpr::Literal(lit) => IrExpr::Literal(IrLiteral {
            value: lit.value.clone(),
            ty: canon_type(&lit.ty, map),
        }),
        IrExpr::Var(var) => IrExpr::Var(IrVar {
            name: var.name.clone(),
            ty: canon_type(&var.ty, map),
        }),
        IrExpr::Let(le) => IrExpr::Let(IrLet {
            name: le.name.clone(),
            ty: canon_type(&le.ty, map),
            value: Box::new(canon_expr(&le.value, map)),
            body: Box::new(canon_expr(&le.body, map)),
        }),
        IrExpr::Lambda(lambda) => IrExpr::Lambda(IrLambda {
            params: lambda.params.iter().map(|p| canon_param(p, map)).collect(),
            body: Box::new(canon_expr(&lambda.body, map)),
            body_type: canon_type(&lambda.body_type, map),
        }),
        IrExpr::App(app) => IrExpr::App(IrApp {
            func: Box::new(canon_expr(&app.func, map)),
            args: app.args.iter().map(|a| canon_expr(a, map)).collect(),
            result_type: canon_type(&app.result_type, map),
        }),
        IrExpr::Match(m) => IrExpr::Match(IrMatch {
            scrutinee: Box::new(canon_expr(&m.scrutinee, map)),
            scrutinee_type: canon_type(&m.scrutinee_type, map),
            arms: m
                .arms
                .iter()
                .map(|arm| IrArm {
                    pattern: canon_pattern(&arm.pattern, map),
                    body: canon_expr(&arm.body, map),
                })
                .collect(),
            result_type: canon_type(&m.result_type, map),
        }),
        IrExpr::Constructor(ctor) => IrExpr::Constructor(IrConstructor {
            name: ctor.name.clone(),
            type_name: ctor.type_name.clone(),
            args: ctor.args.iter().map(|a| canon_expr(a, map)).collect(),
            result_type: canon_type(&ctor.result_type, map),
        }),
        IrExpr::Tuple(tuple) => IrExpr::Tuple(IrTuple {
            elems: tuple.elems.iter().map(|e| canon_expr(e, map)).collect(),
            ty: canon_type(&tuple.ty, map),
        }),
        IrExpr::List(list) => IrExpr::List(IrList {
            elems: list.elems.iter().map(|e| canon_expr(e, map)).collect(),
            ty: canon_type(&list.ty, map),
        }),
        IrExpr::Record(record) => IrExpr::Record(IrRecord {
            fields: record
                .fields
                .iter()
                .map(|f| IrRecordField {
                    label: f.label.clone(),
                    value: canon_expr(&f.value, map),
                })
                .collect(),
            ty: canon_type(&record.ty, map),
        }),
        IrExpr::Field(field) => IrExpr::Field(IrField {
            receiver: Box::new(canon_expr(&field.receiver, map)),
            field: field.field.clone(),
            ty: canon_type(&field.ty, map),
        }),
    }
}

/// Normalises every type in a pattern.
fn canon_pattern(pattern: &IrPattern, map: &mut VarMap) -> IrPattern {
    match pattern {
        IrPattern::Wildcard(w) => IrPattern::Wildcard(IrWildcardPat {
            ty: canon_type(&w.ty, map),
        }),
        IrPattern::Bind(b) => IrPattern::Bind(IrBindPat {
            name: b.name.clone(),
            ty: canon_type(&b.ty, map),
        }),
        IrPattern::Literal(l) => IrPattern::Literal(IrLiteralPat {
            value: l.value.clone(),
            ty: canon_type(&l.ty, map),
        }),
        IrPattern::Tuple(t) => IrPattern::Tuple(IrTuplePat {
            elems: t.elems.iter().map(|e| canon_pattern(e, map)).collect(),
            ty: canon_type(&t.ty, map),
        }),
        IrPattern::Constructor(c) => IrPattern::Constructor(IrConstructorPat {
            name: c.name.clone(),
            type_name: c.type_name.clone(),
            fields: c.fields.iter().map(|f| canon_pattern(f, map)).collect(),
            ty: canon_type(&c.ty, map),
        }),
    }
}

// ── hand-written round-trip programs ─────────────────────────────
//
// Each exercises distinct IR node kinds; together they cover every kind plus
// the desugarings (operators, `if`) and the printer's parenthesisation.

#[test]
fn operators_and_params() {
    assert_roundtrips("fn add(x: Int, y: Int) -> Int = x + y");
}

#[test]
fn polymorphic_signature_skolems() {
    // A signature variable used in two positions, written out of canonical
    // order to exercise the printer's per-signature renumbering.
    assert_roundtrips("fn snd(x: b, y: a) -> a = y");
}

#[test]
fn if_desugars_and_round_trips() {
    assert_roundtrips("fn pick(b: Bool) -> Int = if b then 1 else 2");
}

#[test]
fn adt_match_and_constructors() {
    assert_roundtrips(
        "type Option<a> = Some(a) | None\n\
         fn unwrap(opt: Option<Int>) -> Int = match opt { Some(x) -> x, None -> 0, }",
    );
}

#[test]
fn recursive_adt_and_nested_constructors() {
    assert_roundtrips(
        "type List<a> = Cons(a, List<a>) | Nil\n\
         fn build() -> List<Int> = Cons(1, Cons(2, Nil))",
    );
}

#[test]
fn let_polymorphic_lambda_and_application() {
    assert_roundtrips(r"fn use_id() -> Int = let id = \x -> x in id(1)");
}

#[test]
fn multi_parameter_lambda() {
    assert_roundtrips(r"fn const_fst() -> Int = (\x y -> x)(1, 2)");
}

#[test]
fn tuples_lists_unit_and_literals() {
    assert_roundtrips(
        "fn triple() -> (Int, String, Bool) = (1, \"a\", True)\n\
         fn nums() -> List<Int> = [1, 2, 3]\n\
         fn nothing() = ()\n\
         fn pi() -> Float = 3.14",
    );
}

#[test]
fn zero_argument_function_as_value() {
    // `get`'s return type is the zero-ary `() → Int`, which has no annotation
    // syntax (`() → Int` would re-parse as the one-argument `(()) → Int`), so
    // the printer must omit the return annotation and let inference recover it.
    assert_roundtrips("fn answer() -> Int = 42\nfn get() = answer");
}

#[test]
fn record_literal_and_field_access() {
    // `make` returns a record, whose type has no annotation syntax, so the
    // return annotation is omitted; `age` reads a field off a let-bound record.
    assert_roundtrips(
        "fn make() = { name: \"x\", age: 1 }\n\
         fn age() -> Int = let r = { name: \"x\", age: 1 } in r.age",
    );
}

#[test]
fn extern_reference() {
    assert_roundtrips("extern fn sqrt(x: Float) -> Float");
}

#[test]
fn polymorphic_extern() {
    assert_roundtrips("extern fn identity(x: a) -> a");
}

#[test]
fn literal_and_wildcard_patterns() {
    assert_roundtrips("fn classify(n: Int) -> Int = match n { 0 -> 100, 1 -> 200, _ -> 0, }");
}

#[test]
fn tuple_pattern() {
    assert_roundtrips("fn first(p: (Int, String)) -> Int = match p { (a, b) -> a, }");
}

#[test]
fn nested_operator_precedence() {
    // Mixed precedence and a non-associative comparison; the printer must
    // re-parenthesise to recover the same tree.
    assert_roundtrips("fn prec(a: Int, b: Int) -> Bool = (a + b) * 2 - 1 == b / 2");
}

#[test]
fn qualified_let_bindings() {
    // Sequential `let`s nest to the right; the printer must keep them parseable.
    assert_roundtrips(
        "fn chain() -> Int = let a = 1 in let b = a + 1 in let c = b + 1 in a + b + c",
    );
}

// ── pretty-printer snapshots ─────────────────────────────────────

#[test]
fn snapshot_adt_and_match() {
    let module = lower_src(
        "type Option<a> = Some(a) | None\n\
         fn unwrap(opt: Option<Int>) -> Int = match opt { Some(x) -> x, None -> 0, }",
        "Opt",
    );
    insta::assert_snapshot!(pretty_print(&module));
}

#[test]
fn snapshot_let_lambda_and_operators() {
    let module = lower_src(
        r"fn compute(a: Int, b: Int) -> Int = let scaled = (a + b) * 2 in scaled - 1",
        "Calc",
    );
    insta::assert_snapshot!(pretty_print(&module));
}

#[test]
fn snapshot_polymorphic_and_extern() {
    let module = lower_src(
        "extern fn map(f: a -> b, xs: List<a>) -> List<b>\n\
         fn snd(x: b, y: a) -> a = y",
        "Poly",
    );
    insta::assert_snapshot!(pretty_print(&module));
}

// ── generated round-trip programs ────────────────────────────────
//
// Terms are built type-directed: each generator node knows the type it
// produces, so the rendered `fn main` is well-typed by construction. Binder
// names come from a counter, so no two binders collide.

/// The scalar types terms are generated at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    /// `Int`.
    Int,
    /// `String`.
    Str,
    /// `Bool`.
    Bool,
}

/// A well-typed term, tagged with the type each node produces.
#[derive(Debug, Clone)]
enum Term {
    /// An integer literal.
    IntLit(u8),
    /// A string literal `"s<n>"`.
    StrLit(u8),
    /// A `Bool` constructor.
    BoolLit(bool),
    /// `if c then a else b`, both branches at the target type.
    If(Box<Self>, Box<Self>, Box<Self>),
    /// Integer addition.
    Add(Box<Self>, Box<Self>),
    /// Integer comparison producing `Bool`.
    Lt(Box<Self>, Box<Self>),
    /// Polymorphic equality at the carried operand type.
    Eq(Box<Self>, Box<Self>),
    /// `let v<n> = value in body`; the binder is deliberately unused.
    Let(Box<Self>, Box<Self>),
    /// The identity lambda applied immediately: `(\p<n> -> p<n>)(e)`.
    IdApp(Box<Self>),
    /// A polymorphic let used at the target type:
    /// `let f<n> = \x<n> -> x<n> in f<n>(e)`.
    PolyLet(Box<Self>),
    /// Tuple destructuring: `match (1, e) { (a<n>, b<n>) -> b<n>, }`.
    MatchSnd(Box<Self>),
}

/// A leaf term of type `ty`.
fn leaf(ty: Ty) -> BoxedStrategy<Term> {
    match ty {
        Ty::Int => (0..100_u8).prop_map(Term::IntLit).boxed(),
        Ty::Str => (0..10_u8).prop_map(Term::StrLit).boxed(),
        Ty::Bool => any::<bool>().prop_map(Term::BoolLit).boxed(),
    }
}

/// A term of type `ty` with nesting bounded by `depth`.
fn term(ty: Ty, depth: u32) -> BoxedStrategy<Term> {
    if depth == 0 {
        return leaf(ty);
    }
    let d = depth - 1;
    let mut options: Vec<BoxedStrategy<Term>> = vec![
        leaf(ty),
        (term(Ty::Bool, d), term(ty, d), term(ty, d))
            .prop_map(|(c, a, b)| Term::If(Box::new(c), Box::new(a), Box::new(b)))
            .boxed(),
        (any_ty(), term(ty, d))
            .prop_flat_map(move |(value_ty, body)| {
                term(value_ty, d)
                    .prop_map(move |value| Term::Let(Box::new(value), Box::new(body.clone())))
            })
            .boxed(),
        term(ty, d).prop_map(|e| Term::IdApp(Box::new(e))).boxed(),
        term(ty, d).prop_map(|e| Term::PolyLet(Box::new(e))).boxed(),
        term(ty, d)
            .prop_map(|e| Term::MatchSnd(Box::new(e)))
            .boxed(),
    ];
    match ty {
        Ty::Int => options.push(
            (term(Ty::Int, d), term(Ty::Int, d))
                .prop_map(|(l, r)| Term::Add(Box::new(l), Box::new(r)))
                .boxed(),
        ),
        Ty::Bool => {
            options.push(
                (term(Ty::Int, d), term(Ty::Int, d))
                    .prop_map(|(l, r)| Term::Lt(Box::new(l), Box::new(r)))
                    .boxed(),
            );
            options.push(
                any_ty()
                    .prop_flat_map(move |operand_ty| {
                        (term(operand_ty, d), term(operand_ty, d))
                            .prop_map(|(l, r)| Term::Eq(Box::new(l), Box::new(r)))
                    })
                    .boxed(),
            );
        }
        Ty::Str => {}
    }
    proptest::strategy::Union::new(options).boxed()
}

/// One of the three scalar types.
fn any_ty() -> BoxedStrategy<Ty> {
    prop_oneof![Just(Ty::Int), Just(Ty::Str), Just(Ty::Bool)].boxed()
}

/// Renders `term` to surface syntax, drawing binder names from `next`. Every
/// composite is parenthesised, so the generated source's precedence is never
/// in question (the printer's own parenthesisation is exercised separately).
fn render(term: &Term, next: &mut u32, out: &mut String) {
    use std::fmt::Write as _;

    /// The next unique binder suffix.
    fn bump(next: &mut u32) -> u32 {
        let n = *next;
        *next += 1;
        n
    }

    match term {
        Term::IntLit(v) => write!(out, "{v}").unwrap(),
        Term::StrLit(v) => write!(out, "\"s{v}\"").unwrap(),
        Term::BoolLit(true) => out.push_str("True"),
        Term::BoolLit(false) => out.push_str("False"),
        Term::If(c, a, b) => {
            out.push_str("(if ");
            render(c, next, out);
            out.push_str(" then ");
            render(a, next, out);
            out.push_str(" else ");
            render(b, next, out);
            out.push(')');
        }
        Term::Add(l, r) | Term::Lt(l, r) | Term::Eq(l, r) => {
            let op = match term {
                Term::Add(..) => "+",
                Term::Lt(..) => "<",
                _ => "==",
            };
            out.push('(');
            render(l, next, out);
            write!(out, " {op} ").unwrap();
            render(r, next, out);
            out.push(')');
        }
        Term::Let(value, body) => {
            let n = bump(next);
            write!(out, "(let v{n} = ").unwrap();
            render(value, next, out);
            out.push_str(" in ");
            render(body, next, out);
            out.push(')');
        }
        Term::IdApp(e) => {
            let n = bump(next);
            write!(out, "((\\p{n} -> p{n})(").unwrap();
            render(e, next, out);
            out.push_str("))");
        }
        Term::PolyLet(e) => {
            let n = bump(next);
            write!(out, "(let f{n} = \\x{n} -> x{n} in f{n}(").unwrap();
            render(e, next, out);
            out.push_str("))");
        }
        Term::MatchSnd(e) => {
            let n = bump(next);
            out.push_str("(match (1, ");
            render(e, next, out);
            write!(out, ") {{ (a{n}, b{n}) -> b{n}, }})").unwrap();
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_programs_round_trip(t in any_ty().prop_flat_map(|ty| term(ty, 3))) {
        let mut source = String::from("fn main() = ");
        let mut next = 0;
        render(&t, &mut next, &mut source);
        assert_roundtrips(&source);
    }
}

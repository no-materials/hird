// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Property tests: randomly generated well-typed terms must infer
//! successfully, and must infer the type they were generated at.
//!
//! Terms are built type-directed: each generator node knows the type it
//! produces, so the rendered source is well-typed by construction. Binders
//! are named from a global counter during rendering, so no shadowing
//! warnings arise.

use std::fmt::Write;

use hird_ast::{AstNode, SourceFile};
use proptest::prelude::*;

/// The scalar types terms are generated at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    Int,
    Str,
    Bool,
}

impl Ty {
    /// The surface name of the type.
    fn name(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Str => "String",
            Self::Bool => "Bool",
        }
    }
}

/// A well-typed term, tagged with the type each node produces.
#[derive(Debug, Clone)]
enum Term {
    /// An integer literal.
    IntLit(u8),
    /// A string literal `"s<n>"`.
    StrLit(u8),
    /// A Bool constructor.
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

/// Renders `term` to surface syntax, drawing binder names from `next`.
/// Every composite is parenthesised, so operator precedence never matters.
fn render(term: &Term, next: &mut u32, out: &mut String) {
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
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn well_typed_terms_infer_their_target_type(
        (ty, t) in any_ty().prop_flat_map(|ty| term(ty, 3).prop_map(move |t| (ty, t)))
    ) {
        let mut source = String::from("fn main() = ");
        let mut next = 0;
        render(&t, &mut next, &mut source);

        let parsed = hird_parse::parse(&source, 0);
        prop_assert!(
            parsed.is_ok(),
            "generated source fails to parse: {source}\n{:?}",
            parsed.diagnostics()
        );
        let file = SourceFile::cast(parsed.syntax().clone()).expect("root is a source file");
        let checked = hird_check::check(&file, 0);
        prop_assert!(
            !checked.has_errors(),
            "well-typed term fails to check: {source}\n{:#?}",
            checked.diagnostics
        );
        let main_ty = checked.bindings["main"].to_string();
        prop_assert_eq!(
            main_ty,
            format!("() \u{2192} {}", ty.name()),
            "inferred a different type for: {}", source
        );
    }
}

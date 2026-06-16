// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Pattern-match exhaustiveness and redundancy checking.
//!
//! Implements Maranget's usefulness algorithm over a pattern matrix. A match
//! is non-exhaustive when a wildcard row is *useful* against the matrix of its
//! arms — there is a value no arm matches — and the witnesses reconstructed
//! from that check name the missing cases. An arm is redundant when its own
//! row is *not* useful against the rows above it: every value it matches is
//! already covered.
//!
//! Only finite constructor sets are closed signatures: declared (or seeded)
//! ADTs and tuples. `Int`/`Float`/`String`, type variables, records, and
//! function types are open, so a match over them is exhaustive only through a
//! wildcard or variable arm.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_ast::{AstNode, MatchExpr, Pattern};
use hird_lex::Span;
use hird_types::{Name, Type, unify};

use crate::checker::Checker;
use crate::diag::{CheckCode, CheckDiagnostic};
use crate::node_span;

/// A constructor head used to index a column of the pattern matrix.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Ctor {
    /// A named data constructor of a declared (or seeded) ADT.
    Variant(Name),
    /// The sole constructor of a tuple type.
    Tuple,
    /// A literal, keyed by source text. Its signature is open (infinite).
    Lit(String),
}

/// A pattern reduced to what coverage needs. Wildcard and variable patterns
/// both collapse to [`Pat::Wild`] — the binding does not affect coverage.
#[derive(Clone, Debug)]
enum Pat {
    /// Matches any value of the column type.
    Wild,
    /// A constructor applied to one sub-pattern per field.
    Con(Ctor, Vec<Self>),
}

/// A reconstructed counter-example: a value shape that no arm matches.
#[derive(Clone, Debug)]
enum Witness {
    /// Any value of the column type; renders as `_`.
    Wild,
    /// A constructor applied to one sub-witness per field.
    Con(Ctor, Vec<Self>),
}

/// Lowers a surface pattern to a matrix [`Pat`]. Returns `None` on a malformed
/// CST (a missing name or literal token); the caller then skips the match,
/// since the type pass has already reported the underlying problem.
fn lower(pattern: &Pattern) -> Option<Pat> {
    let pat = match pattern {
        Pattern::Wildcard(_) | Pattern::Bind(_) => Pat::Wild,
        Pattern::Literal(lit) => {
            Pat::Con(Ctor::Lit(String::from(lit.literal()?.text())), Vec::new())
        }
        Pattern::Tuple(tuple) => {
            let mut elems = Vec::new();
            for elem in tuple.elements() {
                elems.push(lower(&elem)?);
            }
            Pat::Con(Ctor::Tuple, elems)
        }
        Pattern::Constructor(ctor) => {
            let mut fields = Vec::new();
            for field in ctor.fields() {
                fields.push(lower(&field)?);
            }
            Pat::Con(Ctor::Variant(Name::new(ctor.name()?)), fields)
        }
    };
    Some(pat)
}

/// Specialises `matrix` by `ctor` of the given `arity`: rows headed by `ctor`
/// keep its sub-patterns, wildcard rows expand to `arity` wildcards, and rows
/// headed by any other constructor are dropped. The first column is consumed.
fn specialize(matrix: &[Vec<Pat>], ctor: &Ctor, arity: usize) -> Vec<Vec<Pat>> {
    let mut out = Vec::new();
    for row in matrix {
        let (head, tail) = row.split_first().expect("matrix row has a column");
        match head {
            Pat::Con(c, args) if c == ctor => {
                let mut new_row = args.clone();
                new_row.extend_from_slice(tail);
                out.push(new_row);
            }
            Pat::Con(_, _) => {}
            Pat::Wild => {
                let mut new_row = alloc::vec![Pat::Wild; arity];
                new_row.extend_from_slice(tail);
                out.push(new_row);
            }
        }
    }
    out
}

/// The default matrix: rows headed by a wildcard, with that head removed.
fn default_matrix(matrix: &[Vec<Pat>]) -> Vec<Vec<Pat>> {
    matrix
        .iter()
        .filter_map(|row| {
            let (head, tail) = row.split_first().expect("matrix row has a column");
            matches!(head, Pat::Wild).then(|| tail.to_vec())
        })
        .collect()
}

/// The distinct constructor heads appearing in the first column of `matrix`.
fn head_ctors(matrix: &[Vec<Pat>]) -> Vec<Ctor> {
    let mut ctors: Vec<Ctor> = Vec::new();
    for row in matrix {
        if let Some(Pat::Con(c, _)) = row.first()
            && !ctors.contains(c)
        {
            ctors.push(c.clone());
        }
    }
    ctors
}

/// Re-wraps witness rows produced after specialising by `ctor` of `arity`:
/// the leading `arity` columns become the constructor's sub-witnesses.
fn wrap_ctor(ctor: &Ctor, arity: usize, rows: Vec<Vec<Witness>>) -> Vec<Vec<Witness>> {
    rows.into_iter()
        .map(|row| {
            let mut iter = row.into_iter();
            let fields: Vec<Witness> = iter.by_ref().take(arity).collect();
            let mut wrapped = alloc::vec![Witness::Con(ctor.clone(), fields)];
            wrapped.extend(iter);
            wrapped
        })
        .collect()
}

/// The field count a constructor scheme exposes (0 for nullary constructors).
fn scheme_arity(scheme: &Type) -> usize {
    match scheme {
        Type::TyForall(_, inner) => scheme_arity(inner),
        Type::TyFn(params, _) => params.len(),
        _ => 0,
    }
}

/// Renders a witness as surface-like syntax for the diagnostic.
fn render_witness(witness: &Witness) -> String {
    match witness {
        Witness::Wild => String::from("_"),
        Witness::Con(Ctor::Lit(text), _) => text.clone(),
        Witness::Con(Ctor::Tuple, args) => {
            let inner: Vec<String> = args.iter().map(render_witness).collect();
            format!("({})", inner.join(", "))
        }
        Witness::Con(Ctor::Variant(name), args) => {
            if args.is_empty() {
                String::from(name.as_str())
            } else {
                let inner: Vec<String> = args.iter().map(render_witness).collect();
                format!("{name}({})", inner.join(", "))
            }
        }
    }
}

/// Builds the C0015 message from reconstructed witnesses: a missing-case list,
/// or — when the only witness is a bare wildcard (an open type) — a prompt to
/// add a catch-all.
fn non_exhaustive_message(witnesses: &[Vec<Witness>]) -> String {
    let mut cases: Vec<String> = witnesses
        .iter()
        .map(|row| {
            let cols: Vec<String> = row.iter().map(render_witness).collect();
            cols.join(", ")
        })
        .collect();
    cases.sort();
    cases.dedup();
    if cases.iter().all(|case| case == "_") {
        return String::from(
            "non-exhaustive match: add a wildcard `_` arm to cover the remaining values",
        );
    }
    const MAX: usize = 6;
    let extra = cases.len().saturating_sub(MAX);
    let mut shown: Vec<String> = cases
        .iter()
        .take(MAX)
        .map(|case| format!("`{case}`"))
        .collect();
    if extra > 0 {
        shown.push(format!("and {extra} more"));
    }
    format!(
        "non-exhaustive match: missing case(s): {}",
        shown.join(", ")
    )
}

impl Checker {
    /// Reports redundant arms (warnings) and a non-exhaustive match (error).
    ///
    /// Call once the scrutinee and every arm have type-checked cleanly: a
    /// match with an ill-typed arm has already aborted, and reporting coverage
    /// on a broken match would only add noise.
    pub(crate) fn check_match(&mut self, me: &MatchExpr, scrutinee: &Type) {
        let span = node_span(me.syntax(), self.source_id);
        let mut arms: Vec<(Pat, Span)> = Vec::new();
        for arm in me.arms() {
            // A missing pattern would already have aborted inference; a pattern
            // that will not lower means a malformed CST — skip either way.
            let Some(pattern) = arm.pattern() else { return };
            let Some(pat) = lower(&pattern) else { return };
            arms.push((pat, node_span(pattern.syntax(), self.source_id)));
        }

        // Redundancy: every arm must be useful against the arms above it.
        let mut rows: Vec<Vec<Pat>> = Vec::new();
        for (pat, arm_span) in arms {
            let row = alloc::vec![pat];
            if self
                .useful(&rows, &row, core::slice::from_ref(scrutinee), span)
                .is_empty()
            {
                self.diags.push(CheckDiagnostic::warning(
                    CheckCode::C0016,
                    arm_span,
                    String::from("unreachable match arm: this pattern is already covered"),
                ));
            }
            rows.push(row);
        }

        // Exhaustiveness: a wildcard must not be useful against all arms.
        let witnesses = self.useful(&rows, &[Pat::Wild], core::slice::from_ref(scrutinee), span);
        if !witnesses.is_empty() {
            self.diags.push(CheckDiagnostic::error(
                CheckCode::C0015,
                span,
                non_exhaustive_message(&witnesses),
            ));
        }
    }

    /// Maranget's usefulness with witness reconstruction: the value shapes `q`
    /// admits that no row of `matrix` does. An empty result means `q` is not
    /// useful (every value it matches is already covered).
    ///
    /// `col_types` gives each column's type, in step with the pattern vectors;
    /// every row and `q` have `col_types.len()` columns.
    fn useful(
        &mut self,
        matrix: &[Vec<Pat>],
        q: &[Pat],
        col_types: &[Type],
        span: Span,
    ) -> Vec<Vec<Witness>> {
        let Some((col_ty, rest_types)) = col_types.split_first() else {
            // Base case: with no columns, `q` is useful iff no row remains.
            return if matrix.is_empty() {
                alloc::vec![Vec::new()]
            } else {
                Vec::new()
            };
        };
        let (head, q_rest) = q.split_first().expect("q has a column per type");
        match head {
            Pat::Con(ctor, args) => {
                let fields = self.field_types(ctor, col_ty, span);
                let arity = fields.len();
                let specialized = specialize(matrix, ctor, arity);
                let mut sub_q = args.clone();
                sub_q.extend_from_slice(q_rest);
                let mut sub_types = fields;
                sub_types.extend_from_slice(rest_types);
                let sub = self.useful(&specialized, &sub_q, &sub_types, span);
                wrap_ctor(ctor, arity, sub)
            }
            Pat::Wild => {
                let present = head_ctors(matrix);
                match self.signature(col_ty) {
                    Some(all) if all.iter().all(|(ctor, _)| present.contains(ctor)) => {
                        // Complete signature: useful iff some constructor's
                        // specialisation is. Recurse into each and collect.
                        let mut out = Vec::new();
                        for (ctor, _) in &all {
                            let fields = self.field_types(ctor, col_ty, span);
                            let arity = fields.len();
                            let specialized = specialize(matrix, ctor, arity);
                            let mut sub_q = alloc::vec![Pat::Wild; arity];
                            sub_q.extend_from_slice(q_rest);
                            let mut sub_types = fields;
                            sub_types.extend_from_slice(rest_types);
                            let sub = self.useful(&specialized, &sub_q, &sub_types, span);
                            out.extend(wrap_ctor(ctor, arity, sub));
                        }
                        out
                    }
                    signature => {
                        // Open, or closed with constructors missing: recurse on
                        // the default matrix, then head each witness with a
                        // missing constructor (closed) or a wildcard (open).
                        let sub = self.useful(&default_matrix(matrix), q_rest, rest_types, span);
                        if sub.is_empty() {
                            return Vec::new();
                        }
                        let heads: Vec<Witness> = match signature {
                            Some(all) => all
                                .into_iter()
                                .filter(|(ctor, _)| !present.contains(ctor))
                                .map(|(ctor, arity)| {
                                    Witness::Con(ctor, alloc::vec![Witness::Wild; arity])
                                })
                                .collect(),
                            None => alloc::vec![Witness::Wild],
                        };
                        let mut out = Vec::new();
                        for witness_head in &heads {
                            for tail in &sub {
                                let mut row = alloc::vec![witness_head.clone()];
                                row.extend_from_slice(tail);
                                out.push(row);
                            }
                        }
                        out
                    }
                }
            }
        }
    }

    /// The closed constructor set of `ty`, each paired with its arity, or
    /// `None` when `ty`'s value space is open.
    fn signature(&self, ty: &Type) -> Option<Vec<(Ctor, usize)>> {
        match self.subst.resolve(ty) {
            Type::TyTuple(elems) => Some(alloc::vec![(Ctor::Tuple, elems.len())]),
            Type::TyCon(name, _) => {
                let ctors = self.registry.adt_constructors(name.as_str())?;
                Some(
                    ctors
                        .iter()
                        .map(|ctor| (Ctor::Variant(ctor.clone()), self.ctor_arity(ctor.as_str())))
                        .collect(),
                )
            }
            _ => None,
        }
    }

    /// The field count of constructor `name` (0 if nullary or undeclared).
    fn ctor_arity(&self, name: &str) -> usize {
        self.registry
            .ctor(name)
            .map_or(0, |info| scheme_arity(&info.scheme))
    }

    /// The field types of `ctor` at column type `col_ty`, in order.
    ///
    /// Tuples read their element types directly. A variant instantiates its
    /// scheme and unifies the result with `col_ty`; because `col_ty` is
    /// concrete here, that unification only binds the fresh instantiation
    /// variables and cannot disturb the surrounding inference state.
    fn field_types(&mut self, ctor: &Ctor, col_ty: &Type, span: Span) -> Vec<Type> {
        match ctor {
            Ctor::Tuple => match self.subst.resolve(col_ty) {
                Type::TyTuple(elems) => elems,
                _ => Vec::new(),
            },
            Ctor::Lit(_) => Vec::new(),
            Ctor::Variant(name) => {
                let Some(info) = self.registry.ctor(name.as_str()) else {
                    return Vec::new();
                };
                match self.subst.instantiate(&info.scheme.clone()) {
                    Type::TyFn(params, ret) => {
                        let _ = unify(&mut self.subst, &ret, col_ty, span);
                        params.iter().map(|p| self.subst.resolve(p)).collect()
                    }
                    other => {
                        let _ = unify(&mut self.subst, &other, col_ty, span);
                        Vec::new()
                    }
                }
            }
        }
    }
}

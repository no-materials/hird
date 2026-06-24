// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The unification algorithm over [`Type`].

use alloc::boxed::Box;
use alloc::vec::Vec;

use hird_lex::Span;

use crate::effect::{Effect, EffectRow};
use crate::error::TypeError;
use crate::subst::Subst;
use crate::ty::Type;

/// Unifies `expected` with `got` under `subst`, recording any variable
/// bindings the equality demands.
///
/// `span` is attached to whichever error the failure produces. The two
/// arguments are symmetric except in [`TypeError::TypeMismatch`], where they
/// fill the `expected` and `got` fields respectively.
///
/// Inputs are expected to be monomorphic: a [`Type::TyForall`] presented for
/// unification is a caller error (it must be instantiated first) and yields
/// [`TypeError::QuantifiedType`] rather than unifying under the binder.
pub fn unify(subst: &mut Subst, expected: &Type, got: &Type, span: Span) -> Result<(), TypeError> {
    let a = subst.head(expected);
    let b = subst.head(got);
    match (a.as_ref(), b.as_ref()) {
        // Quantified types must be instantiated before they reach unification.
        (Type::TyForall(..), _) | (_, Type::TyForall(..)) => {
            Err(TypeError::QuantifiedType { span })
        }
        (Type::TyVar(x), Type::TyVar(y)) => {
            subst.union(*x, *y);
            Ok(())
        }
        (Type::TyVar(x), other) | (other, Type::TyVar(x)) => subst.bind(*x, other.clone(), span),
        (Type::TyCon(n1, args1), Type::TyCon(n2, args2)) => {
            if n1 != n2 || args1.len() != args2.len() {
                return Err(mismatch(subst, expected, got, span));
            }
            for (l, r) in args1.iter().zip(args2.iter()) {
                unify(subst, l, r, span)?;
            }
            Ok(())
        }
        (Type::TyFn(params1, ret1, row1), Type::TyFn(params2, ret2, row2)) => {
            if params1.len() != params2.len() {
                return Err(mismatch(subst, expected, got, span));
            }
            for (l, r) in params1.iter().zip(params2.iter()) {
                unify(subst, l, r, span)?;
            }
            unify(subst, ret1, ret2, span)?;
            unify_row(subst, row1, row2, span)
        }
        (Type::TyTuple(xs), Type::TyTuple(ys)) => {
            if xs.len() != ys.len() {
                return Err(mismatch(subst, expected, got, span));
            }
            for (l, r) in xs.iter().zip(ys.iter()) {
                unify(subst, l, r, span)?;
            }
            Ok(())
        }
        (Type::TyRecord(f1), Type::TyRecord(f2)) => {
            if !f1.keys().eq(f2.keys()) {
                return Err(mismatch(subst, expected, got, span));
            }
            for (l, r) in f1.values().zip(f2.values()) {
                unify(subst, l, r, span)?;
            }
            Ok(())
        }
        _ => Err(mismatch(subst, expected, got, span)),
    }
}

/// Builds a [`TypeError::TypeMismatch`] reporting the fully resolved forms of
/// `expected` and `got`, so diagnostics show solved types rather than raw
/// variables.
fn mismatch(subst: &Subst, expected: &Type, got: &Type, span: Span) -> TypeError {
    TypeError::TypeMismatch {
        expected: Box::new(subst.resolve(expected)),
        got: Box::new(subst.resolve(got)),
        span,
    }
}

/// Unifies effect row `expected` with `got` under `subst`.
///
/// Both rows are resolved first (tails flattened, arguments resolved). Effects
/// sharing a head then have their type arguments unified; effects present on
/// only one side become a *residual* the other side must absorb through its
/// tail. A closed row cannot absorb a residual (a mismatch); an open row binds
/// its tail to the residual; two open rows split a fresh shared tail (the
/// row-variable splitting that makes effect-polymorphic functions work).
///
/// Termination: each call performs finitely many type-argument unifications
/// (on strictly smaller types) and binds row variables, strictly reducing the
/// number of unsolved row variables. The open/open case introduces one fresh
/// tail but binds two existing variables, so the count still falls. With the
/// occurs check on tails rejecting cyclic rows, the recursion is well-founded.
pub fn unify_row(
    subst: &mut Subst,
    expected: &EffectRow,
    got: &EffectRow,
    span: Span,
) -> Result<(), TypeError> {
    let r1 = subst.resolve_row(expected);
    let r2 = subst.resolve_row(got);

    // Unify the arguments of same-head effects; collect the per-side surplus.
    let mut only_in_1 = EffectRow::empty();
    let mut only_in_2 = EffectRow::empty();
    unify_shared_heads(subst, &r1, &r2, span, &mut only_in_1, &mut only_in_2)?;

    match (r1.tail(), r2.tail()) {
        // Closed/closed: every effect must match; any surplus is a mismatch.
        (None, None) => {
            if only_in_1.is_empty() && only_in_2.is_empty() {
                Ok(())
            } else {
                Err(effect_mismatch(&r1, &r2, &only_in_1, &only_in_2, span))
            }
        }
        // Open/closed: the open side's surplus has nowhere to go in the closed
        // side; the closed side's surplus fills the open tail (closed).
        (Some(t1), None) => {
            if only_in_1.is_empty() {
                subst.row_bind(t1, only_in_2.with_tail(None), span)
            } else {
                Err(effect_mismatch(&r1, &r2, &only_in_1, &only_in_2, span))
            }
        }
        (None, Some(t2)) => {
            if only_in_2.is_empty() {
                subst.row_bind(t2, only_in_1.with_tail(None), span)
            } else {
                Err(effect_mismatch(&r1, &r2, &only_in_1, &only_in_2, span))
            }
        }
        // Open/open: a shared fresh tail absorbs both surpluses. The same tail
        // variable on both sides instead demands the surpluses be empty.
        (Some(t1), Some(t2)) => {
            if t1 == t2 {
                if only_in_1.is_empty() && only_in_2.is_empty() {
                    Ok(())
                } else {
                    Err(effect_mismatch(&r1, &r2, &only_in_1, &only_in_2, span))
                }
            } else if only_in_1.is_empty() && only_in_2.is_empty() {
                // No surplus on either side: the rows agree up to their tails,
                // so equate the tails directly.
                subst.row_union(t1, t2);
                Ok(())
            } else {
                let fresh = subst.fresh_row();
                subst.row_bind(t1, only_in_2.with_tail(Some(fresh)), span)?;
                subst.row_bind(t2, only_in_1.with_tail(Some(fresh)), span)
            }
        }
    }
}

/// Unifies the arguments of effects that share a head between the two resolved
/// rows, accumulating into `only_in_1`/`only_in_2` the effects that appear on
/// just one side.
fn unify_shared_heads(
    subst: &mut Subst,
    r1: &EffectRow,
    r2: &EffectRow,
    span: Span,
    only_in_1: &mut EffectRow,
    only_in_2: &mut EffectRow,
) -> Result<(), TypeError> {
    for (head, l1) in r1.buckets() {
        match r2.buckets().get(head) {
            Some(l2) => unify_head_bucket(subst, l1, l2, span, only_in_1, only_in_2)?,
            None => {
                for effect in l1 {
                    only_in_1.insert(effect.clone());
                }
            }
        }
    }
    for (head, l2) in r2.buckets() {
        if !r1.buckets().contains_key(head) {
            for effect in l2 {
                only_in_2.insert(effect.clone());
            }
        }
    }
    Ok(())
}

/// Unifies the two effect lists sharing one head. Structurally-equal effects
/// (already-ground duplicates) cancel; a lone effect on each side has its
/// arguments unified; anything left over is routed to the surplus rows.
///
/// Pairing several distinct same-head effects precisely needs multiset
/// machinery, which v0.1 does not build: the overlap is paired positionally and
/// the rest becomes surplus. The common shapes — one effect per head, or equal
/// ground sets — are handled exactly.
fn unify_head_bucket(
    subst: &mut Subst,
    l1: &[Effect],
    l2: &[Effect],
    span: Span,
    only_in_1: &mut EffectRow,
    only_in_2: &mut EffectRow,
) -> Result<(), TypeError> {
    let mut rem1: Vec<&Effect> = Vec::new();
    let mut rem2: Vec<&Effect> = l2.iter().collect();
    for e1 in l1 {
        if let Some(pos) = rem2.iter().position(|e2| *e2 == e1) {
            // A structurally-equal effect on both sides: cancel the pair.
            rem2.remove(pos);
        } else {
            rem1.push(e1);
        }
    }
    let overlap = rem1.len().min(rem2.len());
    for (e1, e2) in rem1.iter().zip(&rem2) {
        unify_effect_args(subst, e1, e2, span)?;
    }
    for effect in &rem1[overlap..] {
        only_in_1.insert((*effect).clone());
    }
    for effect in &rem2[overlap..] {
        only_in_2.insert((*effect).clone());
    }
    Ok(())
}

/// Unifies the type arguments of two same-head effects pairwise. A differing
/// argument count is an effect mismatch naming the offending effect.
fn unify_effect_args(
    subst: &mut Subst,
    expected: &Effect,
    got: &Effect,
    span: Span,
) -> Result<(), TypeError> {
    let a1 = expected.args();
    let a2 = got.args();
    if a1.len() != a2.len() {
        return Err(TypeError::EffectMismatch {
            expected: Box::new(EffectRow::closed([expected.clone()])),
            got: Box::new(EffectRow::closed([got.clone()])),
            offending: Some(expected.clone()),
            span,
        });
    }
    for (x, y) in a1.iter().zip(a2) {
        unify(subst, x, y, span)?;
    }
    Ok(())
}

/// Builds an [`TypeError::EffectMismatch`] over the resolved rows, naming the
/// first surplus effect as the offending one.
fn effect_mismatch(
    expected: &EffectRow,
    got: &EffectRow,
    only_in_1: &EffectRow,
    only_in_2: &EffectRow,
    span: Span,
) -> TypeError {
    let offending = only_in_1
        .effects()
        .chain(only_in_2.effects())
        .next()
        .cloned();
    TypeError::EffectMismatch {
        expected: Box::new(expected.clone()),
        got: Box::new(got.clone()),
        offending,
        span,
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use hird_lex::Span;

    use super::{unify, unify_row};
    use crate::effect::{Effect, EffectRow};
    use crate::error::TypeError;
    use crate::subst::Subst;
    use crate::ty::Type;

    // Docstring notation: α, β are unification variables. `~` is "unify",
    // `⇒` the outcome, `{α ↦ T}` a recorded solution, `∅` no solutions,
    // `⊥` failure, `μα. T` an infinite type.

    /// A throwaway span; these tests never inspect span contents.
    fn span() -> Span {
        Span::new(0, 0, 0)
    }

    // -- primitives --------------------------------------------------------

    /// `Int ~ Int ⇒ ∅`
    #[test]
    fn int_unifies_with_int() {
        let mut s = Subst::new();
        assert!(unify(&mut s, &Type::int(), &Type::int(), span()).is_ok());
    }

    /// `Int ~ String ⇒ ⊥`
    #[test]
    fn int_mismatches_string() {
        let mut s = Subst::new();
        let err = unify(&mut s, &Type::int(), &Type::string(), span()).unwrap_err();
        let TypeError::TypeMismatch { expected, got, .. } = err else {
            panic!("expected a TypeMismatch, got {err:?}");
        };
        assert_eq!(*expected, Type::int());
        assert_eq!(*got, Type::string());
    }

    // -- variables ---------------------------------------------------------

    /// `α ~ Int ⇒ {α ↦ Int}`
    #[test]
    fn var_binds_to_int() {
        let mut s = Subst::new();
        let a = s.fresh();
        unify(&mut s, &Type::var(a), &Type::int(), span()).unwrap();
        assert_eq!(s.resolve(&Type::var(a)), Type::int());
    }

    /// `Int ~ α ⇒ {α ↦ Int}` — unification is symmetric.
    #[test]
    fn var_binds_when_on_the_right() {
        let mut s = Subst::new();
        let a = s.fresh();
        unify(&mut s, &Type::int(), &Type::var(a), span()).unwrap();
        assert_eq!(s.resolve(&Type::var(a)), Type::int());
    }

    /// `α ~ β, β ~ Int ⇒ {α ↦ Int, β ↦ Int}`
    #[test]
    fn transitive_binding_through_union_find() {
        let mut s = Subst::new();
        let a = s.fresh();
        let b = s.fresh();
        unify(&mut s, &Type::var(a), &Type::var(b), span()).unwrap();
        unify(&mut s, &Type::var(b), &Type::int(), span()).unwrap();
        assert_eq!(s.resolve(&Type::var(a)), Type::int());
        assert_eq!(s.resolve(&Type::var(b)), Type::int());
    }

    // -- functions ---------------------------------------------------------

    /// `(α → β) ~ (Int → String) ⇒ {α ↦ Int, β ↦ String}`
    #[test]
    fn function_binds_both_sides() {
        let mut s = Subst::new();
        let a = s.fresh();
        let b = s.fresh();
        let lhs = Type::func(vec![Type::var(a)], Type::var(b));
        let rhs = Type::func(vec![Type::int()], Type::string());
        unify(&mut s, &lhs, &rhs, span()).unwrap();
        assert_eq!(s.resolve(&Type::var(a)), Type::int());
        assert_eq!(s.resolve(&Type::var(b)), Type::string());
        assert_eq!(s.resolve(&lhs), rhs);
    }

    /// `(Int → Bool) ~ (String → Bool) ⇒ ⊥` — reported at `Int ~ String`.
    #[test]
    fn function_reports_inner_mismatch() {
        let mut s = Subst::new();
        let lhs = Type::func(vec![Type::int()], Type::bool());
        let rhs = Type::func(vec![Type::string()], Type::bool());
        let err = unify(&mut s, &lhs, &rhs, span()).unwrap_err();
        let TypeError::TypeMismatch { expected, got, .. } = err else {
            panic!("expected a TypeMismatch, got {err:?}");
        };
        assert_eq!(*expected, Type::int());
        assert_eq!(*got, Type::string());
    }

    /// `(Int → Int) ~ (Int → Int → Int) ⇒ ⊥` — arity 1 ≠ 2.
    #[test]
    fn function_arity_mismatch_fails() {
        let mut s = Subst::new();
        let lhs = Type::func(vec![Type::int()], Type::int());
        let rhs = Type::func(vec![Type::int(), Type::int()], Type::int());
        let err = unify(&mut s, &lhs, &rhs, span()).unwrap_err();
        assert!(matches!(err, TypeError::TypeMismatch { .. }));
    }

    // -- occurs check ------------------------------------------------------

    /// `α ~ List<α> ⇒ ⊥` — would require the infinite type `μα. List<α>`.
    #[test]
    fn occurs_check_rejects_infinite_type() {
        let mut s = Subst::new();
        let a = s.fresh();
        let recursive = Type::list(Type::var(a));
        let err = unify(&mut s, &Type::var(a), &recursive, span()).unwrap_err();
        let TypeError::InfiniteType { var, in_type, .. } = err else {
            panic!("expected an InfiniteType, got {err:?}");
        };
        assert_eq!(var, a);
        assert_eq!(*in_type, recursive);
    }

    /// `α ~ β, β ~ List<α> ⇒ ⊥` — occurrence is checked on the class
    /// `{α, β}`, not the variable's spelling.
    #[test]
    fn occurs_check_sees_through_substitution() {
        let mut s = Subst::new();
        let a = s.fresh();
        let b = s.fresh();
        unify(&mut s, &Type::var(a), &Type::var(b), span()).unwrap();
        let err = unify(&mut s, &Type::var(b), &Type::list(Type::var(a)), span()).unwrap_err();
        assert!(matches!(err, TypeError::InfiniteType { .. }));
    }

    // -- tuples ------------------------------------------------------------

    /// `(α, Int) ~ (Bool, β) ⇒ {α ↦ Bool, β ↦ Int}`
    #[test]
    fn tuple_unifies_componentwise() {
        let mut s = Subst::new();
        let a = s.fresh();
        let b = s.fresh();
        let lhs = Type::tuple(vec![Type::var(a), Type::int()]);
        let rhs = Type::tuple(vec![Type::bool(), Type::var(b)]);
        unify(&mut s, &lhs, &rhs, span()).unwrap();
        assert_eq!(s.resolve(&Type::var(a)), Type::bool());
        assert_eq!(s.resolve(&Type::var(b)), Type::int());
    }

    /// `(Int, Int) ~ (Int, Int, Int) ⇒ ⊥`
    #[test]
    fn tuple_arity_mismatch_fails() {
        let mut s = Subst::new();
        let lhs = Type::tuple(vec![Type::int(), Type::int()]);
        let rhs = Type::tuple(vec![Type::int(), Type::int(), Type::int()]);
        let err = unify(&mut s, &lhs, &rhs, span()).unwrap_err();
        assert!(matches!(err, TypeError::TypeMismatch { .. }));
    }

    // -- records -----------------------------------------------------------

    /// `{ x: α, y: Int } ~ { x: Bool, y: β } ⇒ {α ↦ Bool, β ↦ Int}`
    #[test]
    fn record_unifies_structurally() {
        let mut s = Subst::new();
        let a = s.fresh();
        let b = s.fresh();
        let lhs = Type::record([
            (crate::Label::new("x"), Type::var(a)),
            (crate::Label::new("y"), Type::int()),
        ]);
        let rhs = Type::record([
            (crate::Label::new("x"), Type::bool()),
            (crate::Label::new("y"), Type::var(b)),
        ]);
        unify(&mut s, &lhs, &rhs, span()).unwrap();
        assert_eq!(s.resolve(&Type::var(a)), Type::bool());
        assert_eq!(s.resolve(&Type::var(b)), Type::int());
    }

    /// `{ x: Int } ~ { y: Int } ⇒ ⊥`
    #[test]
    fn record_label_mismatch_fails() {
        let mut s = Subst::new();
        let lhs = Type::record([(crate::Label::new("x"), Type::int())]);
        let rhs = Type::record([(crate::Label::new("y"), Type::int())]);
        let err = unify(&mut s, &lhs, &rhs, span()).unwrap_err();
        assert!(matches!(err, TypeError::TypeMismatch { .. }));
    }

    /// `{ x: Int } ~ { x: Int, y: Int } ⇒ ⊥` — label sets must match
    /// exactly; no row polymorphism until effect rows land.
    #[test]
    fn record_extra_label_fails() {
        let mut s = Subst::new();
        let lhs = Type::record([(crate::Label::new("x"), Type::int())]);
        let rhs = Type::record([
            (crate::Label::new("x"), Type::int()),
            (crate::Label::new("y"), Type::int()),
        ]);
        let err = unify(&mut s, &lhs, &rhs, span()).unwrap_err();
        assert!(matches!(err, TypeError::TypeMismatch { .. }));
    }

    // -- constructors ------------------------------------------------------

    /// `List<α> ~ List<Int> ⇒ {α ↦ Int}`
    #[test]
    fn constructor_args_unify() {
        let mut s = Subst::new();
        let a = s.fresh();
        unify(
            &mut s,
            &Type::list(Type::var(a)),
            &Type::list(Type::int()),
            span(),
        )
        .unwrap();
        assert_eq!(s.resolve(&Type::var(a)), Type::int());
    }

    /// `List<Int> ~ Option<Int> ⇒ ⊥`
    #[test]
    fn constructor_name_mismatch_fails() {
        let mut s = Subst::new();
        let err = unify(
            &mut s,
            &Type::list(Type::int()),
            &Type::option(Type::int()),
            span(),
        )
        .unwrap_err();
        assert!(matches!(err, TypeError::TypeMismatch { .. }));
    }

    /// `Map<Int, Int> ~ Map<Int> ⇒ ⊥`
    #[test]
    fn constructor_arity_mismatch_fails() {
        let mut s = Subst::new();
        let lhs = Type::con("Map", vec![Type::int(), Type::int()]);
        let rhs = Type::con("Map", vec![Type::int()]);
        let err = unify(&mut s, &lhs, &rhs, span()).unwrap_err();
        assert!(matches!(err, TypeError::TypeMismatch { .. }));
    }

    // -- quantified precondition ------------------------------------------

    /// `(∀α. α) ~ Int ⇒ ⊥` — schemes must be instantiated before they
    /// reach unification; this failure flags a caller bug, not a program
    /// type error.
    #[test]
    fn quantified_type_is_rejected() {
        let mut s = Subst::new();
        let forall = Type::TyForall(vec![0], vec![], alloc::boxed::Box::new(Type::var(0)));
        assert!(matches!(
            unify(&mut s, &forall, &Type::int(), span()),
            Err(TypeError::QuantifiedType { .. }),
        ));
        assert!(matches!(
            unify(&mut s, &Type::int(), &forall, span()),
            Err(TypeError::QuantifiedType { .. }),
        ));
    }

    // -- effect rows -------------------------------------------------------
    //
    // Notation mirrors the surface syntax: `{Log}` a closed row, `{Log | r}`
    // an open one. A parametric effect `Tool<X>` carries a type argument.

    /// The nullary effect `Name`.
    fn named(name: &str) -> Effect {
        Effect::named(name)
    }

    /// The parametric effect `Tool<Con>` over a nullary constructor.
    fn tool(con: &str) -> Effect {
        Effect::parametric("Tool", vec![Type::con(con, vec![])])
    }

    /// `{Log | r1} ~ {Log | r2}` ⇒ the shared head is not duplicated and the
    /// tails are merged. Written first and run repeatedly: a buggy residual
    /// split here loops forever under re-resolution, so idempotence is the
    /// canary for termination.
    #[test]
    fn open_open_overlapping_head_is_idempotent() {
        let mut s = Subst::new();
        let r1 = s.fresh_row();
        let r2 = s.fresh_row();
        let a = EffectRow::open([named("Log")], r1);
        let b = EffectRow::open([named("Log")], r2);
        unify_row(&mut s, &a, &b, span()).unwrap();

        let ra = s.resolve_row(&a);
        let rb = s.resolve_row(&b);
        assert_eq!(ra, rb, "both rows resolve to the same row");
        assert_eq!(ra.effects().count(), 1, "Log is not duplicated");
        assert!(ra.tail().is_some(), "the row stays open");

        // Re-unifying is a no-op that terminates and changes nothing.
        unify_row(&mut s, &a, &b, span()).unwrap();
        assert_eq!(s.resolve_row(&a), ra);
    }

    /// `{Log} ~ {Log} ⇒ ∅` — equal closed rows unify trivially.
    #[test]
    fn closed_rows_equal() {
        let mut s = Subst::new();
        let a = EffectRow::closed([named("Log")]);
        assert!(unify_row(&mut s, &a, &a.clone(), span()).is_ok());
    }

    /// `{Log} ~ {Spawn} ⇒ ⊥` — different closed rows do not unify.
    #[test]
    fn closed_rows_differ() {
        let mut s = Subst::new();
        let a = EffectRow::closed([named("Log")]);
        let b = EffectRow::closed([named("Spawn")]);
        assert!(matches!(
            unify_row(&mut s, &a, &b, span()),
            Err(TypeError::EffectMismatch { .. })
        ));
    }

    /// `{Log, Spawn} ~ {Log} ⇒ ⊥` — a closed row cannot drop an effect.
    #[test]
    fn closed_row_missing_effect() {
        let mut s = Subst::new();
        let a = EffectRow::closed([named("Log"), named("Spawn")]);
        let b = EffectRow::closed([named("Log")]);
        assert!(matches!(
            unify_row(&mut s, &a, &b, span()),
            Err(TypeError::EffectMismatch { .. })
        ));
    }

    /// `{Log | r} ~ {Log, Tool<X>} ⇒ r ↦ {Tool<X>}` — an open row's tail
    /// absorbs the closed row's surplus.
    #[test]
    fn open_closed_binds_tail() {
        let mut s = Subst::new();
        let r = s.fresh_row();
        let open = EffectRow::open([named("Log")], r);
        let closed = EffectRow::closed([named("Log"), tool("X")]);
        unify_row(&mut s, &open, &closed, span()).unwrap();
        assert_eq!(s.resolve_row(&open), s.resolve_row(&closed));
        assert_eq!(
            s.resolve_row(&open),
            EffectRow::closed([named("Log"), tool("X")])
        );
    }

    /// `{r} ~ {Log} ⇒ r ↦ {Log}` — a bare row variable solves to a closed row.
    #[test]
    fn bare_variable_binds_to_closed() {
        let mut s = Subst::new();
        let r = s.fresh_row();
        let open = EffectRow::of_var(r);
        let closed = EffectRow::closed([named("Log")]);
        unify_row(&mut s, &open, &closed, span()).unwrap();
        assert_eq!(s.resolve_row(&open), closed);
    }

    /// `{Log, Spawn | r} ~ {Log} ⇒ ⊥` — the open side's surplus has no home in
    /// the closed side.
    #[test]
    fn open_surplus_against_closed_fails() {
        let mut s = Subst::new();
        let r = s.fresh_row();
        let open = EffectRow::open([named("Log"), named("Spawn")], r);
        let closed = EffectRow::closed([named("Log")]);
        assert!(matches!(
            unify_row(&mut s, &open, &closed, span()),
            Err(TypeError::EffectMismatch { .. })
        ));
    }

    /// `{Log | r1} ~ {Tool<X> | r2}` ⇒ both rows resolve to
    /// `{Log, Tool<X> | r3}` — the row-variable split.
    #[test]
    fn open_open_splits_residual() {
        let mut s = Subst::new();
        let r1 = s.fresh_row();
        let r2 = s.fresh_row();
        let a = EffectRow::open([named("Log")], r1);
        let b = EffectRow::open([tool("X")], r2);
        unify_row(&mut s, &a, &b, span()).unwrap();
        let ra = s.resolve_row(&a);
        assert_eq!(ra, s.resolve_row(&b), "both rows agree after the split");
        assert_eq!(ra.effects().count(), 2, "both heads are present");
        assert!(ra.tail().is_some(), "a shared fresh tail remains");
    }

    /// `{Tool<a>} ~ {Tool<Int>} ⇒ a ↦ Int` — same-head effects unify their
    /// type arguments.
    #[test]
    fn parametric_effect_unifies_arguments() {
        let mut s = Subst::new();
        let a = s.fresh_type();
        let lhs = EffectRow::closed([Effect::parametric("Tool", vec![a.clone()])]);
        let rhs = EffectRow::closed([tool("Int")]);
        unify_row(&mut s, &lhs, &rhs, span()).unwrap();
        assert_eq!(s.resolve(&a), Type::con("Int", vec![]));
    }

    /// `{Tool<X>} ~ {Tool<Y>} ⇒ ⊥` — same head, so the arguments are unified
    /// and the failure surfaces as the underlying type mismatch (`X ~ Y`).
    #[test]
    fn parametric_effect_argument_mismatch() {
        let mut s = Subst::new();
        let lhs = EffectRow::closed([tool("X")]);
        let rhs = EffectRow::closed([tool("Y")]);
        assert!(matches!(
            unify_row(&mut s, &lhs, &rhs, span()),
            Err(TypeError::TypeMismatch { .. })
        ));
    }

    /// `(Int → Int ! {Log}) ~ (Int → Int ! {r}) ⇒ r ↦ {Log}` — unifying
    /// function types unifies their effect rows.
    #[test]
    fn function_types_unify_effect_rows() {
        let mut s = Subst::new();
        let r = s.fresh_row();
        let concrete = Type::func_eff(
            vec![Type::int()],
            Type::int(),
            EffectRow::closed([named("Log")]),
        );
        let polymorphic = Type::func_eff(vec![Type::int()], Type::int(), EffectRow::of_var(r));
        unify(&mut s, &concrete, &polymorphic, span()).unwrap();
        assert_eq!(
            s.resolve_row(&EffectRow::of_var(r)),
            EffectRow::closed([named("Log")])
        );
    }

    /// Binding `r ↦ {Log | r}` is rejected: the tail mentions `r`, an infinite
    /// row. The occurs check is a guard the splitting algorithm never trips,
    /// so it is exercised directly.
    #[test]
    fn row_occurs_check_rejects_infinite_row() {
        let mut s = Subst::new();
        let r = s.fresh_row();
        let cyclic = EffectRow::open([named("Log")], r);
        assert!(matches!(
            s.row_bind(r, cyclic, span()),
            Err(TypeError::InfiniteEffectRow { .. })
        ));
    }

    /// A second open/open unification reusing a solved tail still terminates
    /// and stays consistent — a regression guard for the termination measure.
    #[test]
    fn chained_open_unifications_terminate() {
        let mut s = Subst::new();
        let r1 = s.fresh_row();
        let r2 = s.fresh_row();
        let r3 = s.fresh_row();
        let a = EffectRow::open([named("Log")], r1);
        let b = EffectRow::open([tool("X")], r2);
        let c = EffectRow::open([named("Spawn")], r3);
        unify_row(&mut s, &a, &b, span()).unwrap();
        unify_row(&mut s, &a, &c, span()).unwrap();
        // All three rows now share the same resolved effects.
        let ra = s.resolve_row(&a);
        assert_eq!(ra, s.resolve_row(&b));
        assert_eq!(ra, s.resolve_row(&c));
        assert!(
            ra.effects()
                .any(|e| matches!(e, Effect::Named(n) if n.as_str() == "Log"))
        );
        assert!(
            ra.effects()
                .any(|e| matches!(e, Effect::Named(n) if n.as_str() == "Spawn"))
        );
    }

    /// `{r} ~ {r}` — the same tail on both sides needs no binding and does not
    /// loop.
    #[test]
    fn identical_open_rows_unify() {
        let mut s = Subst::new();
        let r = s.fresh_row();
        let a = EffectRow::open([named("Log")], r);
        assert!(unify_row(&mut s, &a, &a.clone(), span()).is_ok());
    }
}

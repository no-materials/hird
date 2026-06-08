// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The unification algorithm over [`Type`].

use hird_lex::Span;

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
    match (a, b) {
        // Quantified types must be instantiated before they reach unification.
        (Type::TyForall(..), _) | (_, Type::TyForall(..)) => {
            Err(TypeError::QuantifiedType { span })
        }
        (Type::TyVar(x), Type::TyVar(y)) => {
            subst.union(x, y);
            Ok(())
        }
        (Type::TyVar(x), other) | (other, Type::TyVar(x)) => subst.bind(x, other, span),
        (Type::TyCon(n1, args1), Type::TyCon(n2, args2)) => {
            if n1 != n2 || args1.len() != args2.len() {
                return Err(mismatch(subst, expected, got, span));
            }
            for (l, r) in args1.iter().zip(args2.iter()) {
                unify(subst, l, r, span)?;
            }
            Ok(())
        }
        (Type::TyFn(from1, to1), Type::TyFn(from2, to2)) => {
            unify(subst, &from1, &from2, span)?;
            unify(subst, &to1, &to2, span)
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
        expected: subst.resolve(expected),
        got: subst.resolve(got),
        span,
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use hird_lex::Span;

    use super::unify;
    use crate::error::TypeError;
    use crate::subst::Subst;
    use crate::ty::Type;

    /// A throwaway span; these tests never inspect span contents.
    fn span() -> Span {
        Span::new(0, 0, 0)
    }

    // -- primitives --------------------------------------------------------

    #[test]
    fn int_unifies_with_int() {
        let mut s = Subst::new();
        assert!(unify(&mut s, &Type::int(), &Type::int(), span()).is_ok());
    }

    #[test]
    fn int_mismatches_string() {
        let mut s = Subst::new();
        let err = unify(&mut s, &Type::int(), &Type::string(), span()).unwrap_err();
        let TypeError::TypeMismatch { expected, got, .. } = err else {
            panic!("expected a TypeMismatch, got {err:?}");
        };
        assert_eq!(expected, Type::int());
        assert_eq!(got, Type::string());
    }

    // -- variables ---------------------------------------------------------

    #[test]
    fn var_binds_to_int() {
        let mut s = Subst::new();
        let a = s.fresh();
        unify(&mut s, &Type::var(a), &Type::int(), span()).unwrap();
        assert_eq!(s.resolve(&Type::var(a)), Type::int());
    }

    #[test]
    fn var_binds_when_on_the_right() {
        let mut s = Subst::new();
        let a = s.fresh();
        unify(&mut s, &Type::int(), &Type::var(a), span()).unwrap();
        assert_eq!(s.resolve(&Type::var(a)), Type::int());
    }

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

    #[test]
    fn function_binds_both_sides() {
        let mut s = Subst::new();
        let a = s.fresh();
        let b = s.fresh();
        let lhs = Type::func(Type::var(a), Type::var(b));
        let rhs = Type::func(Type::int(), Type::string());
        unify(&mut s, &lhs, &rhs, span()).unwrap();
        assert_eq!(s.resolve(&Type::var(a)), Type::int());
        assert_eq!(s.resolve(&Type::var(b)), Type::string());
        assert_eq!(s.resolve(&lhs), rhs);
    }

    #[test]
    fn function_reports_inner_mismatch() {
        let mut s = Subst::new();
        let lhs = Type::func(Type::int(), Type::bool());
        let rhs = Type::func(Type::string(), Type::bool());
        let err = unify(&mut s, &lhs, &rhs, span()).unwrap_err();
        let TypeError::TypeMismatch { expected, got, .. } = err else {
            panic!("expected a TypeMismatch, got {err:?}");
        };
        assert_eq!(expected, Type::int());
        assert_eq!(got, Type::string());
    }

    // -- occurs check ------------------------------------------------------

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
        assert_eq!(in_type, recursive);
    }

    #[test]
    fn occurs_check_sees_through_substitution() {
        let mut s = Subst::new();
        let a = s.fresh();
        let b = s.fresh();
        // After equating `a` and `b`, binding `b` to `List<a>` is still infinite.
        unify(&mut s, &Type::var(a), &Type::var(b), span()).unwrap();
        let err = unify(&mut s, &Type::var(b), &Type::list(Type::var(a)), span()).unwrap_err();
        assert!(matches!(err, TypeError::InfiniteType { .. }));
    }

    // -- tuples ------------------------------------------------------------

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

    #[test]
    fn tuple_arity_mismatch_fails() {
        let mut s = Subst::new();
        let lhs = Type::tuple(vec![Type::int(), Type::int()]);
        let rhs = Type::tuple(vec![Type::int(), Type::int(), Type::int()]);
        let err = unify(&mut s, &lhs, &rhs, span()).unwrap_err();
        assert!(matches!(err, TypeError::TypeMismatch { .. }));
    }

    // -- records -----------------------------------------------------------

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

    #[test]
    fn record_label_mismatch_fails() {
        let mut s = Subst::new();
        let lhs = Type::record([(crate::Label::new("x"), Type::int())]);
        let rhs = Type::record([(crate::Label::new("y"), Type::int())]);
        let err = unify(&mut s, &lhs, &rhs, span()).unwrap_err();
        assert!(matches!(err, TypeError::TypeMismatch { .. }));
    }

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

    #[test]
    fn constructor_arity_mismatch_fails() {
        let mut s = Subst::new();
        let lhs = Type::con("Map", vec![Type::int(), Type::int()]);
        let rhs = Type::con("Map", vec![Type::int()]);
        let err = unify(&mut s, &lhs, &rhs, span()).unwrap_err();
        assert!(matches!(err, TypeError::TypeMismatch { .. }));
    }

    // -- quantified precondition ------------------------------------------

    #[test]
    fn quantified_type_is_rejected() {
        let mut s = Subst::new();
        let forall = Type::TyForall(vec![0], alloc::boxed::Box::new(Type::var(0)));
        assert!(matches!(
            unify(&mut s, &forall, &Type::int(), span()),
            Err(TypeError::QuantifiedType { .. }),
        ));
        assert!(matches!(
            unify(&mut s, &Type::int(), &forall, span()),
            Err(TypeError::QuantifiedType { .. }),
        ));
    }
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hirð type representation, unification, and scheme operations.
//!
//! This crate is the type-system core: the [`Type`] representation, a
//! union-find [`Subst`]itution table with level-tracked generalisation and
//! instantiation, and [`unify`]ation with an occurs check. It is pure data
//! and algorithms — lowering from the AST and expression inference live in
//! `hird-check`.
//!
//! # Quick start
//!
//! ```
//! use hird_lex::Span;
//! use hird_types::{Subst, Type, unify};
//!
//! let mut subst = Subst::new();
//! let a = subst.fresh_type();
//! let span = Span::new(0, 0, 0);
//!
//! // Unifying `a` with `Int` solves `a`.
//! unify(&mut subst, &a, &Type::int(), span).unwrap();
//! assert_eq!(subst.resolve(&a), Type::int());
//! ```

#![no_std]

extern crate alloc;

mod effect;
mod error;
mod name;
mod subst;
mod ty;
mod unify;

pub use effect::{Effect, EffectRow, RowVar};
pub use error::TypeError;
pub use name::{Label, Name};
pub use subst::Subst;
pub use ty::Type;
pub use unify::{unify, unify_row};

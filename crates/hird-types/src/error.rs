// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Errors produced by unification.
//!
//! Plain `no_std` data carrying the spans needed for later diagnostic
//! rendering; no rendering logic lives here.

use alloc::boxed::Box;

use hird_lex::Span;

use crate::effect::{Effect, EffectRow, RowVar};
use crate::ty::Type;

/// A unification failure, located at the call site that requested the unification.
///
/// The [`Type`] and [`EffectRow`] payloads are boxed: they are needed only on
/// the cold error path, and boxing keeps `Result<_, TypeError>` small on the
/// hot success path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeError {
    /// Two incompatible types were required to be equal.
    TypeMismatch {
        /// Type demanded by context.
        expected: Box<Type>,
        /// Type actually supplied.
        got: Box<Type>,
        /// Where the requirement arose.
        span: Span,
    },
    /// A variable was unified with a type that contains it, which would
    /// describe an infinitely large type.
    InfiniteType {
        /// The offending variable.
        var: u32,
        /// The type it could not be bound to, resolved for display.
        in_type: Box<Type>,
        /// Where the binding was attempted.
        span: Span,
    },
    /// Two effect rows could not be unified: a closed row was missing an effect
    /// the other required, or two same-head effects had incompatible arguments.
    EffectMismatch {
        /// Row demanded by context, resolved for display.
        expected: Box<EffectRow>,
        /// Row actually supplied, resolved for display.
        got: Box<EffectRow>,
        /// The effect whose absence (from a closed row) blocked unification, if
        /// the failure pins one down.
        offending: Option<Effect>,
        /// Where the requirement arose.
        span: Span,
    },
    /// A row variable was unified with a row that contains it in its tail, which
    /// would describe an infinitely long effect row.
    InfiniteEffectRow {
        /// The offending row variable.
        var: RowVar,
        /// The row it could not be bound to, resolved for display.
        in_row: Box<EffectRow>,
        /// Where the binding was attempted.
        span: Span,
    },
    /// A quantified type reached unification. Generalised types must be
    /// instantiated to a monomorphic form before they are unified; reaching
    /// this arm is a precondition violation by the caller, not a type error
    /// in the program under analysis.
    QuantifiedType {
        /// Where the quantified type was encountered.
        span: Span,
    },
}

impl TypeError {
    /// The source span this error refers to.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::TypeMismatch { span, .. }
            | Self::InfiniteType { span, .. }
            | Self::EffectMismatch { span, .. }
            | Self::InfiniteEffectRow { span, .. }
            | Self::QuantifiedType { span } => *span,
        }
    }
}

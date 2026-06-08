// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Errors produced by unification.
//!
//! Plain `no_std` data carrying the spans needed for later diagnostic
//! rendering; no rendering logic lives here.

use hird_lex::Span;

use crate::ty::Type;

/// A unification failure, located at the call site that requested the unification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeError {
    /// Two incompatible types were required to be equal.
    TypeMismatch {
        /// Type demanded by context.
        expected: Type,
        /// Type actually supplied.
        got: Type,
        /// Where the requirement arose.
        span: Span,
    },
    /// A variable was unified with a type that contains it, which would
    /// describe an infinitely large type.
    InfiniteType {
        /// The offending variable.
        var: u32,
        /// The type it could not be bound to, resolved for display.
        in_type: Type,
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
            | Self::QuantifiedType { span } => *span,
        }
    }
}

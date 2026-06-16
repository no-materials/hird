// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Plain diagnostic types for type-checking errors and warnings.
//!
//! These are `no_std`-compatible data structs with no rendering logic,
//! mirroring the parser's diagnostics. Messages are pre-formatted strings
//! because they interpolate type renderings.

use alloc::format;
use alloc::string::String;

use hird_lex::Span;
use hird_types::{Type, TypeError};

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The program is ill-typed; checking the surrounding declaration stopped.
    Error,
    /// Suspicious but legal; checking continued.
    Warning,
}

/// Machine-readable diagnostic codes for the type checker.
///
/// Codes use a `C` prefix (checker) followed by a four-digit number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CheckCode {
    /// Two incompatible types were required to be equal.
    C0001,
    /// A variable was unified with a type containing it (infinite type).
    C0002,
    /// A value name has no binding in scope.
    C0003,
    /// A type name has no declaration.
    C0004,
    /// A type constructor was applied to the wrong number of arguments.
    C0005,
    /// A function was called with the wrong number of arguments.
    C0006,
    /// A pattern names a constructor that no type declares.
    C0007,
    /// A constructor pattern has the wrong number of fields.
    C0008,
    /// A field access whose receiver is not a known record type.
    C0009,
    /// A record type has no field of the accessed name.
    C0010,
    /// An inner binding shadows an outer one (warning).
    C0011,
    /// A type expression uses a type variable the declaration does not bind.
    C0012,
    /// A type declaration binds the same type parameter twice.
    C0013,
    /// An `extern` declaration is missing part of its signature.
    C0014,
}

/// A single type-checker diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckDiagnostic {
    /// Machine-readable code.
    pub code: CheckCode,
    /// Whether this stops the surrounding declaration's check.
    pub severity: Severity,
    /// Source location the diagnostic points at.
    pub span: Span,
    /// Human-readable message.
    pub message: String,
}

impl CheckDiagnostic {
    /// An error diagnostic.
    #[must_use]
    pub fn error(code: CheckCode, span: Span, message: String) -> Self {
        Self {
            code,
            severity: Severity::Error,
            span,
            message,
        }
    }

    /// A warning diagnostic.
    #[must_use]
    pub fn warning(code: CheckCode, span: Span, message: String) -> Self {
        Self {
            code,
            severity: Severity::Warning,
            span,
            message,
        }
    }

    /// Converts a unification failure into a diagnostic.
    #[must_use]
    pub fn from_type_error(err: &TypeError) -> Self {
        match err {
            TypeError::TypeMismatch {
                expected,
                got,
                span,
            } => Self::error(
                CheckCode::C0001,
                *span,
                format!("type mismatch: expected `{expected}`, got `{got}`"),
            ),
            TypeError::InfiniteType { var, in_type, span } => Self::error(
                CheckCode::C0002,
                *span,
                format!(
                    "infinite type: `{}` occurs within `{in_type}`",
                    Type::var(*var)
                ),
            ),
            // A checker invariant violation, not a program error; surfaced
            // rather than panicking so a compiler bug never takes the session
            // down with it.
            TypeError::QuantifiedType { span } => Self::error(
                CheckCode::C0001,
                *span,
                String::from("internal: quantified type reached unification"),
            ),
        }
    }
}

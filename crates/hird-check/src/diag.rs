// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Plain diagnostic types for type-checking errors and warnings.
//!
//! These are `no_std`-compatible data structs with no rendering logic,
//! mirroring the parser's diagnostics. Messages are pre-formatted strings
//! because they interpolate type renderings.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

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
    /// A match does not cover every value of the scrutinee's type.
    C0015,
    /// A match arm is unreachable: earlier arms already cover it.
    C0016,
    /// A value name (function, binding, or constructor) is defined twice in
    /// one module, or an import collides with such a definition.
    C0017,
    /// A type name is defined twice in one module, or an imported type
    /// collides with a local type.
    C0018,
    /// A module's `module` declaration disagrees with its path-derived name.
    C0019,
    /// Two or more modules import each other, forming a cycle.
    C0020,
    /// A pattern destructures an opaque type outside its declaring module.
    C0021,
    /// A value constructs an opaque type outside its declaring module.
    C0022,
    /// A `use` import names a module that does not exist or a name the target
    /// module does not export.
    C0023,
    /// A qualified name `Mod.member` references a value the module does not
    /// export.
    C0024,
    /// Two effect rows could not be unified.
    C0025,
    /// A row variable was unified with a row that contains it (infinite row).
    C0026,
    /// An effect annotation names an undeclared effect.
    C0027,
    /// An effect is applied to the wrong number of type arguments.
    C0028,
    /// An effect row lists more than one row variable.
    C0029,
    /// A function's body performs effects its declared row omits, or declares
    /// effects the body never performs (the rows are checked for equality).
    C0030,
    /// A `handle` arm's handler expression does not have a function type.
    C0031,
    /// A tool signature contains a type that is not wire-representable: a
    /// function type or an opaque capability.
    C0032,
    /// A `handle` arm handles `Tool<X>` where `X` is not a declared tool.
    C0033,
    /// A `handle` arm's handler does not match the handled tool's operation
    /// signature.
    C0034,
    /// An actor declaration is structurally invalid: a missing, duplicate, or
    /// unknown member, a malformed message type, or a duplicate actor name.
    C0035,
    /// An actor declares two handlers for the same message constructor.
    C0036,
    /// A handler's message pattern does not name a constructor of the actor's
    /// message type.
    C0037,
    /// An actor's declared effect summary does not match the union of its
    /// init and handler effects.
    C0038,
    /// A `spawn` expression names an actor that is not declared, or supplies
    /// the wrong number of init arguments.
    C0039,
    /// An actor is referenced as a value; actor state and members are only
    /// accessible within the actor's handlers.
    C0040,
    /// An actor's handlers do not cover every constructor of its message
    /// type.
    C0041,
    /// A `request` message builder is not a bare message constructor.
    C0042,
    /// A constructor carrying a `ReplyTo` field is used outside a `request`
    /// builder — as an ordinary value or application.
    C0043,
    /// A message constructor nests a `ReplyTo` field (directly or through a
    /// named type) or declares more than one.
    C0044,
    /// A message constructor carries a `ReplyTo` field alongside other fields;
    /// a reply channel must be the constructor's only field.
    C0045,
}

/// A secondary source location attached to a diagnostic.
///
/// Carries the "other" span a message refers to — for a duplicate definition,
/// the original; the primary [`CheckDiagnostic::span`] is the offending site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedSpan {
    /// The related source location.
    pub span: Span,
    /// What this location is, e.g. `first defined here`.
    pub message: String,
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
    /// Secondary locations the message refers to, in attachment order. Empty
    /// for the common single-span diagnostic.
    pub related: Vec<RelatedSpan>,
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
            related: Vec::new(),
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
            related: Vec::new(),
        }
    }

    /// Attaches a secondary location, consuming and returning `self` for
    /// chaining.
    #[must_use]
    pub fn with_related(mut self, span: Span, message: String) -> Self {
        self.related.push(RelatedSpan { span, message });
        self
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
            TypeError::EffectMismatch {
                expected,
                got,
                offending,
                span,
            } => Self::error(
                CheckCode::C0025,
                *span,
                match offending {
                    Some(effect) => format!(
                        "effect mismatch: expected `{expected}`, got `{got}` (effect `{effect}`)"
                    ),
                    None => format!("effect mismatch: expected `{expected}`, got `{got}`"),
                },
            ),
            TypeError::InfiniteEffectRow { var, in_row, span } => Self::error(
                CheckCode::C0026,
                *span,
                format!("infinite effect row: `{var}` occurs within `{in_row}`"),
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

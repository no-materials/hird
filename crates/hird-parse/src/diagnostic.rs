// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Plain diagnostic types for parser errors.
//!
//! These are `no_std`-compatible data structs with no rendering logic.
//! Downstream crates (or the `std` feature) convert them into `miette`
//! diagnostics for terminal display.

use hird_lex::Span;

/// A single parser diagnostic (error or warning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnostic {
    /// Machine-readable error code.
    pub code: DiagnosticCode,
    /// Source location of the error.
    pub span: Span,
    /// Human-readable error message.
    pub message: &'static str,
}

/// Machine-readable diagnostic codes for parser errors.
///
/// Codes use a `P` prefix (parser) followed by a four-digit number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    /// Expected a specific token but found something else.
    P0001,
    /// Unexpected token in this position.
    P0002,
    /// Unterminated delimiter (missing closing paren/brace/bracket).
    P0003,
    /// Incomplete declaration.
    P0004,
    /// Malformed type annotation.
    P0005,
}

impl DiagnosticCode {
    /// Returns the code as a static string (e.g. `"P0001"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::P0001 => "P0001",
            Self::P0002 => "P0002",
            Self::P0003 => "P0003",
            Self::P0004 => "P0004",
            Self::P0005 => "P0005",
        }
    }
}

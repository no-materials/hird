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
    /// Optional suggestion shown beneath the message, e.g. how to fix the
    /// error. `None` when there is no actionable hint.
    pub help: Option<&'static str>,
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
    /// Malformed type annotation.
    P0003,
    /// Nesting depth limit exceeded.
    P0004,
    /// Non-associative operator used in a chain.
    P0005,
    /// A return type written where the language fixes it: an actor handler
    /// (always `Next<State>`) or `init` (always the state type).
    P0006,
    /// `opaque` on a type alias: an alias is a name for a shape, not a type
    /// with constructors to hide, so `pub type alias` is the only exported
    /// form.
    P0007,
    /// A `..base` entry that is not the single, last entry of a record
    /// literal.
    P0008,
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
            Self::P0006 => "P0006",
            Self::P0007 => "P0007",
            Self::P0008 => "P0008",
        }
    }
}

/// `std`-only rendering of `ParseDiagnostic` values as graphical `miette`
/// reports.
#[cfg(feature = "std")]
mod render {
    use alloc::boxed::Box;
    use alloc::string::String;
    use core::fmt;

    use miette::{Diagnostic, GraphicalReportHandler, GraphicalTheme, LabeledSpan, SourceCode};

    use super::ParseDiagnostic;

    /// Internal adapter: a [`ParseDiagnostic`] paired with its source text,
    /// made renderable by implementing [`miette::Diagnostic`].
    #[derive(Debug)]
    struct ParseReport<'a> {
        /// The diagnostic being rendered.
        diagnostic: &'a ParseDiagnostic,
        /// Full source text the diagnostic's span points into.
        source: &'a str,
    }

    impl fmt::Display for ParseReport<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.diagnostic.message)
        }
    }

    impl core::error::Error for ParseReport<'_> {}

    impl Diagnostic for ParseReport<'_> {
        fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
            Some(Box::new(self.diagnostic.code.as_str()))
        }

        fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
            self.diagnostic
                .help
                .map(|help| Box::new(help) as Box<dyn fmt::Display + 'a>)
        }

        fn source_code(&self) -> Option<&dyn SourceCode> {
            Some(&self.source)
        }

        fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
            let span = self.diagnostic.span;
            let label = LabeledSpan::new_primary_with_span(
                None,
                (span.start as usize, span.len() as usize),
            );
            Some(Box::new(core::iter::once(label)))
        }
    }

    /// Renders `diagnostic` against its `source` as a graphical report string,
    /// using a deterministic, uncoloured Unicode theme. The output is stable
    /// across environments — suitable for tests, logs, and non-terminal sinks.
    #[must_use]
    pub fn render(diagnostic: &ParseDiagnostic, source: &str) -> String {
        let report = ParseReport { diagnostic, source };
        let mut out = String::new();
        GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor())
            .render_report(&mut out, &report)
            .expect("writing a report into an owned String is infallible");
        out
    }
}

#[cfg(feature = "std")]
pub use render::render;

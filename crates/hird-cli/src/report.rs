// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Terminal rendering of check diagnostics, mirroring the parser's
//! `miette`-based report style.

use std::fmt;
use std::path::Path;

use hird_check::CheckDiagnostic;
use miette::{
    Diagnostic, GraphicalReportHandler, GraphicalTheme, LabeledSpan, NamedSource, Severity,
    SourceCode,
};

/// Internal adapter: a [`CheckDiagnostic`] paired with its named source text,
/// made renderable by implementing [`miette::Diagnostic`].
#[derive(Debug)]
struct CheckReport<'a> {
    /// The diagnostic being rendered.
    diagnostic: &'a CheckDiagnostic,
    /// The named source the diagnostic's span points into.
    source: NamedSource<String>,
}

impl fmt::Display for CheckReport<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.diagnostic.message)
    }
}

impl std::error::Error for CheckReport<'_> {}

impl Diagnostic for CheckReport<'_> {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(format!("{:?}", self.diagnostic.code)))
    }

    fn severity(&self) -> Option<Severity> {
        Some(match self.diagnostic.severity {
            hird_check::Severity::Error => Severity::Error,
            hird_check::Severity::Warning => Severity::Warning,
        })
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(&self.source)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let span = self.diagnostic.span;
        let primary =
            LabeledSpan::new_primary_with_span(None, (span.start as usize, span.len() as usize));
        // Related spans in other files cannot label this source; they are
        // dropped rather than mislabelled.
        let related = self
            .diagnostic
            .related
            .iter()
            .filter(move |r| r.span.source_id == span.source_id)
            .map(|r| {
                LabeledSpan::new_with_span(
                    Some(r.message.clone()),
                    (r.span.start as usize, r.span.len() as usize),
                )
            });
        Some(Box::new(core::iter::once(primary).chain(related)))
    }
}

/// Renders `diagnostic` against its file as a graphical report string, using
/// the same deterministic, uncoloured Unicode theme as parse diagnostics.
#[must_use]
pub(crate) fn render(diagnostic: &CheckDiagnostic, path: &Path, source: &str) -> String {
    let report = CheckReport {
        diagnostic,
        source: NamedSource::new(path.display().to_string(), source.to_owned()),
    };
    let mut out = String::new();
    GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor())
        .render_report(&mut out, &report)
        .expect("writing a report into an owned String is infallible");
    out
}

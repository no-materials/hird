// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

// Diagnostic rendering lives behind the `std` feature; the test is gated to
// match. Run with `--all-features` to exercise it.
#[cfg(feature = "std")]
#[test]
fn renders_code_span_message_and_help() {
    use hird_lex::Span;
    use hird_parse::diagnostic::{DiagnosticCode, ParseDiagnostic, render};

    let source = "fn f() = a == b == c";
    let diagnostic = ParseDiagnostic {
        code: DiagnosticCode::P0005,
        // The second `==` (bytes 16..18): the token that forms the chain.
        span: Span::new(16, 18, 0),
        message: "non-associative operator cannot be chained; parenthesise",
        help: Some("group the comparisons, e.g. `(a == b) == c`"),
    };

    let rendered = render(&diagnostic, source);

    // The report must surface the code, the primary message, the help text,
    // and a snippet of the offending source.
    assert!(rendered.contains("P0005"), "code missing:\n{rendered}");
    assert!(
        rendered.contains("non-associative operator cannot be chained"),
        "message missing:\n{rendered}"
    );
    assert!(
        rendered.contains("group the comparisons"),
        "help missing:\n{rendered}"
    );
    assert!(
        rendered.contains("=="),
        "source snippet missing:\n{rendered}"
    );

    insta::assert_snapshot!(rendered);
}

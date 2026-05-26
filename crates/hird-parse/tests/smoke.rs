// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

use hird_parse::SyntaxKind;

#[test]
fn parse_trivial_let() {
    let result = hird_parse::parse("let x = 42", 0);
    assert!(result.is_ok());

    let root = cstree::syntax::SyntaxNode::<SyntaxKind>::new_root(result.green().clone());
    assert_eq!(root.kind(), SyntaxKind::SOURCE_FILE);

    // Flat structure for now: SOURCE_FILE contains tokens + whitespace.
    // Verify all source bytes are accounted for.
    let children: Vec<_> = root.children_with_tokens().collect();

    // "let x = 42" should produce:
    // LET_KW "let", WS " ", IDENT "x", WS " ", EQ "=", WS " ", INT "42"
    let kinds: Vec<_> = children
        .iter()
        .map(|c| match c {
            cstree::util::NodeOrToken::Node(n) => n.kind(),
            cstree::util::NodeOrToken::Token(t) => t.kind(),
        })
        .collect();

    assert_eq!(
        kinds,
        vec![
            SyntaxKind::LET_KW,
            SyntaxKind::WHITESPACE,
            SyntaxKind::IDENT,
            SyntaxKind::WHITESPACE,
            SyntaxKind::EQ,
            SyntaxKind::WHITESPACE,
            SyntaxKind::INT,
        ]
    );
}

#[test]
fn parse_empty_source() {
    let result = hird_parse::parse("", 0);
    assert!(result.is_ok());

    let root = cstree::syntax::SyntaxNode::<SyntaxKind>::new_root(result.green().clone());
    assert_eq!(root.kind(), SyntaxKind::SOURCE_FILE);
    assert_eq!(root.children_with_tokens().count(), 0);
}

#[test]
fn parse_whitespace_only() {
    let result = hird_parse::parse("  \n  ", 0);
    assert!(result.is_ok());

    let root = cstree::syntax::SyntaxNode::<SyntaxKind>::new_root(result.green().clone());
    let kinds: Vec<_> = root
        .children_with_tokens()
        .map(|c| match c {
            cstree::util::NodeOrToken::Node(n) => n.kind(),
            cstree::util::NodeOrToken::Token(t) => t.kind(),
        })
        .collect();

    // All whitespace, no real tokens — but the parser should still emit
    // the trailing whitespace.
    assert_eq!(kinds, vec![SyntaxKind::WHITESPACE]);
}

#[test]
fn parse_preserves_comments() {
    let result = hird_parse::parse("// hello\nlet x = 1", 0);
    assert!(result.is_ok());

    let root = cstree::syntax::SyntaxNode::<SyntaxKind>::new_root(result.green().clone());
    let kinds: Vec<_> = root
        .children_with_tokens()
        .map(|c| match c {
            cstree::util::NodeOrToken::Node(n) => n.kind(),
            cstree::util::NodeOrToken::Token(t) => t.kind(),
        })
        .collect();

    assert_eq!(
        kinds,
        vec![
            SyntaxKind::LINE_COMMENT, // "// hello"
            SyntaxKind::WHITESPACE,   // "\n"
            SyntaxKind::LET_KW,       // "let"
            SyntaxKind::WHITESPACE,   // " "
            SyntaxKind::IDENT,        // "x"
            SyntaxKind::WHITESPACE,   // " "
            SyntaxKind::EQ,           // "="
            SyntaxKind::WHITESPACE,   // " "
            SyntaxKind::INT,          // "1"
        ]
    );
}

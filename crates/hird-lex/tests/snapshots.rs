// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

use std::fmt::Write;

use hird_lex::Lexer;

fn snapshot(source: &str) -> String {
    let mut out = String::new();
    for tok in Lexer::new(source, 0) {
        let text = tok.span.text(source);
        let kind = format!("{:?}", tok.kind);
        let _ = writeln!(
            out,
            "{kind:<32} {start:>3}..{end:<3} {text:?}",
            start = tok.span.start,
            end = tok.span.end,
        );
    }
    out
}

// ============================================================
// Category 1: Keyword recognition
// ============================================================

#[test]
fn keywords_all() {
    insta::assert_snapshot!(snapshot(
        "let fn match type actor supervisor effect tool \
         handle spawn send request use module pub extern if then else in",
    ));
}

#[test]
fn keywords_in_context() {
    insta::assert_snapshot!(snapshot("let x = if y then 1 else 0"));
}

#[test]
fn keyword_prefixes_are_idents() {
    insta::assert_snapshot!(snapshot("letter fns matching types actors"));
}

// ============================================================
// Category 2: Operator tokens
// ============================================================

#[test]
fn operators_single_char() {
    insta::assert_snapshot!(snapshot("+ - * / < > = | ! . :"));
}

#[test]
fn operators_multi_char() {
    insta::assert_snapshot!(snapshot("-> => <= >= == != ::"));
}

#[test]
fn delimiters() {
    insta::assert_snapshot!(snapshot("( ) { } [ ] , ;"));
}

// ============================================================
// Category 3: Unicode normalization pairs
// ============================================================

#[test]
fn arrow_normalization() {
    let ascii_kinds: Vec<_> = Lexer::new("a -> b", 0).map(|t| t.kind).collect();
    let unicode_kinds: Vec<_> = Lexer::new("a \u{2192} b", 0).map(|t| t.kind).collect();
    assert_eq!(ascii_kinds, unicode_kinds);

    insta::assert_snapshot!("arrow_ascii", snapshot("a -> b"));
    insta::assert_snapshot!("arrow_unicode", snapshot("a \u{2192} b"));
}

#[test]
fn fat_arrow_normalization() {
    let ascii_kinds: Vec<_> = Lexer::new("x => y", 0).map(|t| t.kind).collect();
    let unicode_kinds: Vec<_> = Lexer::new("x \u{21d2} y", 0).map(|t| t.kind).collect();
    assert_eq!(ascii_kinds, unicode_kinds);

    insta::assert_snapshot!("fat_arrow_ascii", snapshot("x => y"));
    insta::assert_snapshot!("fat_arrow_unicode", snapshot("x \u{21d2} y"));
}

#[test]
fn lambda_normalization() {
    let ascii_kinds: Vec<_> = Lexer::new("\\x", 0).map(|t| t.kind).collect();
    let unicode_kinds: Vec<_> = Lexer::new("\u{03bb}x", 0).map(|t| t.kind).collect();
    assert_eq!(ascii_kinds, unicode_kinds);

    insta::assert_snapshot!("lambda_ascii", snapshot("\\x"));
    insta::assert_snapshot!("lambda_unicode", snapshot("\u{03bb}x"));
}

#[test]
fn mixed_unicode_expression() {
    insta::assert_snapshot!(snapshot("\u{03bb}x \u{2192} x + x"));
}

// ============================================================
// Category 4: String literals
// ============================================================

#[test]
fn string_empty() {
    insta::assert_snapshot!(snapshot(r#""""#));
}

#[test]
fn string_simple() {
    insta::assert_snapshot!(snapshot(r#""hello world""#));
}

#[test]
fn string_escape_sequences() {
    insta::assert_snapshot!(snapshot(r#""tab\there\nnewline\\backslash\"""#));
}

#[test]
fn string_unicode_content() {
    insta::assert_snapshot!(snapshot("\"caf\u{00e9} \u{03bb}\u{2192}\u{21d2}\""));
}

// ============================================================
// Category 5: Numeric literals
// ============================================================

#[test]
fn integer_literals() {
    insta::assert_snapshot!(snapshot("0 42 123456"));
}

#[test]
fn float_literals() {
    insta::assert_snapshot!(snapshot("0.0 3.14 100.5"));
}

#[test]
fn integer_followed_by_dot() {
    insta::assert_snapshot!(snapshot("42. 42.x"));
}

#[test]
fn numbers_in_expression() {
    insta::assert_snapshot!(snapshot("1 + 2.5 * 3"));
}

// ============================================================
// Category 6: Comments
// ============================================================

#[test]
fn comment_line() {
    insta::assert_snapshot!(snapshot("// this is a comment"));
}

#[test]
fn comment_line_before_code() {
    insta::assert_snapshot!(snapshot("// comment\nlet x = 1"));
}

#[test]
fn comment_block() {
    insta::assert_snapshot!(snapshot("/* block comment */"));
}

#[test]
fn comment_block_nested() {
    insta::assert_snapshot!(snapshot("/* outer /* inner */ still outer */"));
}

// ============================================================
// Category 7: Canonical naming violations
// ============================================================

#[test]
fn naming_camel_case_value() {
    insta::assert_snapshot!(snapshot("fooBar camelCase getX"));
}

#[test]
fn naming_pascal_with_underscore() {
    insta::assert_snapshot!(snapshot("Foo_Bar FOO_BAR"));
}

#[test]
fn naming_leading_underscore_violation() {
    insta::assert_snapshot!(snapshot("_fooBar _Foo_Bar __camelCase"));
}

#[test]
fn naming_valid_identifiers() {
    insta::assert_snapshot!(snapshot("foo_bar FooBar x _ __ _x"));
}

// ============================================================
// Category 8: Error recovery
// ============================================================

#[test]
fn error_unterminated_string() {
    insta::assert_snapshot!(snapshot("let x = \"oops\nlet y = 1"));
}

#[test]
fn error_unterminated_block_comment() {
    insta::assert_snapshot!(snapshot("/* oops"));
}

#[test]
fn error_unexpected_char() {
    insta::assert_snapshot!(snapshot("42 @ 7"));
}

#[test]
fn error_multiple_recovery() {
    insta::assert_snapshot!(snapshot("let x = @ + # * 42"));
}

#[test]
fn error_unicode_unexpected() {
    insta::assert_snapshot!(snapshot("x + \u{20ac} * y"));
}

// ============================================================
// Category 9: Full program lexing
// ============================================================

#[test]
fn program_function() {
    insta::assert_snapshot!(snapshot(
        "fn add(x: Int, y: Int) \u{2192} Int ! {} {\n  x + y\n}",
    ));
}

#[test]
fn program_let_with_lambda() {
    insta::assert_snapshot!(snapshot("let double = \u{03bb}x \u{2192} x + x"));
}

#[test]
fn program_module_with_effects() {
    insta::assert_snapshot!(snapshot(
        "module Planner\n\n\
         use Log\n\n\
         effect Tool\n\n\
         pub fn plan(config: Config) \u{2192} Plan {\n\
         \x20 let data = config\n\
         \x20 data\n\
         }",
    ));
}

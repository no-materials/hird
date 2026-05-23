// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hand-written lexer for Hird source.

use crate::token::{LexError, Span, Token, TokenKind};

/// Produces [`Token`]s from Hird source text.
///
/// The lexer is a cursor over `&str` input. It handles Unicode
/// operator normalisation (`->` and `\u{2192}` both produce [`TokenKind::Arrow`]),
/// checks canonical naming conventions at lex time, and recovers
/// from errors by emitting [`TokenKind::Error`] tokens and continuing.
///
/// Use [`next_token`](Self::next_token) for explicit control, or
/// iterate directly (the [`Iterator`] impl yields every token except
/// [`TokenKind::Eof`]).
#[derive(Debug)]
pub struct Lexer<'src> {
    source: &'src str,
    source_id: u32,
    pos: usize,
}

impl<'src> Lexer<'src> {
    /// Creates a new lexer over `source`.
    ///
    /// `source_id` is stored in every [`Span`] the lexer produces so
    /// that downstream passes can attribute spans to specific files.
    #[must_use]
    pub fn new(source: &'src str, source_id: u32) -> Self {
        Self {
            source,
            source_id,
            pos: 0,
        }
    }

    /// Returns the next token, advancing past it.
    ///
    /// Returns [`TokenKind::Eof`] when input is exhausted. Subsequent
    /// calls continue to return `Eof`.
    #[must_use]
    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();

        let start = self.pos;
        let bytes = self.source.as_bytes();

        let Some(&byte) = bytes.get(self.pos) else {
            return self.tok(TokenKind::Eof, start);
        };

        match byte {
            b'(' => self.single(TokenKind::LParen, start),
            b')' => self.single(TokenKind::RParen, start),
            b'{' => self.single(TokenKind::LBrace, start),
            b'}' => self.single(TokenKind::RBrace, start),
            b'[' => self.single(TokenKind::LBracket, start),
            b']' => self.single(TokenKind::RBracket, start),
            b',' => self.single(TokenKind::Comma, start),
            b';' => self.single(TokenKind::Semicolon, start),
            b'+' => self.single(TokenKind::Plus, start),
            b'*' => self.single(TokenKind::Star, start),
            b'|' => self.single(TokenKind::Pipe, start),
            b'.' => self.single(TokenKind::Dot, start),

            b'-' => {
                self.pos += 1;
                if bytes.get(self.pos) == Some(&b'>') {
                    self.pos += 1;
                    self.tok(TokenKind::Arrow, start)
                } else {
                    self.tok(TokenKind::Minus, start)
                }
            }

            b'=' => {
                self.pos += 1;
                match bytes.get(self.pos) {
                    Some(&b'>') => {
                        self.pos += 1;
                        self.tok(TokenKind::FatArrow, start)
                    }
                    Some(&b'=') => {
                        self.pos += 1;
                        self.tok(TokenKind::EqEq, start)
                    }
                    _ => self.tok(TokenKind::Eq, start),
                }
            }

            b'!' => {
                self.pos += 1;
                if bytes.get(self.pos) == Some(&b'=') {
                    self.pos += 1;
                    self.tok(TokenKind::BangEq, start)
                } else {
                    self.tok(TokenKind::Bang, start)
                }
            }

            b'<' => {
                self.pos += 1;
                if bytes.get(self.pos) == Some(&b'=') {
                    self.pos += 1;
                    self.tok(TokenKind::Le, start)
                } else {
                    self.tok(TokenKind::Lt, start)
                }
            }

            b'>' => {
                self.pos += 1;
                if bytes.get(self.pos) == Some(&b'=') {
                    self.pos += 1;
                    self.tok(TokenKind::Ge, start)
                } else {
                    self.tok(TokenKind::Gt, start)
                }
            }

            b':' => {
                self.pos += 1;
                if bytes.get(self.pos) == Some(&b':') {
                    self.pos += 1;
                    self.tok(TokenKind::ColonColon, start)
                } else {
                    self.tok(TokenKind::Colon, start)
                }
            }

            b'\\' => self.single(TokenKind::Lambda, start),

            b'/' => {
                self.pos += 1;
                match bytes.get(self.pos) {
                    Some(&b'/') => self.lex_line_comment(start),
                    Some(&b'*') => self.lex_block_comment(start),
                    _ => self.tok(TokenKind::Slash, start),
                }
            }

            b'"' => self.lex_string(start),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.lex_ident(start),
            b'0'..=b'9' => self.lex_number(start),

            _ => self.lex_unicode_or_error(start),
        }
    }

    // ---- internal helpers ------------------------------------------------

    #[expect(
        clippy::cast_possible_truncation,
        reason = "source files cannot exceed u32::MAX bytes"
    )]
    fn tok(&self, kind: TokenKind, start: usize) -> Token {
        Token {
            kind,
            span: Span::new(start as u32, self.pos as u32, self.source_id),
        }
    }

    fn single(&mut self, kind: TokenKind, start: usize) -> Token {
        self.pos += 1;
        self.tok(kind, start)
    }

    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        let bytes = self.source.as_bytes();
        while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn lex_unicode_or_error(&mut self, start: usize) -> Token {
        let ch = self.source[self.pos..].chars().next().expect("non-empty");
        self.pos += ch.len_utf8();
        let kind = match ch {
            '→' => TokenKind::Arrow,
            '⇒' => TokenKind::FatArrow,
            'λ' => TokenKind::Lambda,
            _ => TokenKind::Error(LexError::UnexpectedChar),
        };
        self.tok(kind, start)
    }

    fn lex_ident(&mut self, start: usize) -> Token {
        let bytes = self.source.as_bytes();
        while self.pos < bytes.len()
            && (bytes[self.pos].is_ascii_alphanumeric() || bytes[self.pos] == b'_')
        {
            self.pos += 1;
        }

        let text = &self.source[start..self.pos];

        let kind = match text {
            "let" => TokenKind::Let,
            "fn" => TokenKind::Fn,
            "match" => TokenKind::Match,
            "type" => TokenKind::Type,
            "actor" => TokenKind::Actor,
            "supervisor" => TokenKind::Supervisor,
            "effect" => TokenKind::Effect,
            "tool" => TokenKind::Tool,
            "handle" => TokenKind::Handle,
            "spawn" => TokenKind::Spawn,
            "send" => TokenKind::Send,
            "request" => TokenKind::Request,
            "use" => TokenKind::Use,
            "module" => TokenKind::Module,
            "pub" => TokenKind::Pub,
            "extern" => TokenKind::Extern,
            "if" => TokenKind::If,
            "then" => TokenKind::Then,
            "else" => TokenKind::Else,
            _ => {
                if has_naming_violation(text) {
                    return self.tok(TokenKind::Error(LexError::NonCanonicalName), start);
                }
                TokenKind::Ident
            }
        };

        self.tok(kind, start)
    }

    fn lex_number(&mut self, start: usize) -> Token {
        let bytes = self.source.as_bytes();
        while self.pos < bytes.len() && bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }

        if self.pos < bytes.len()
            && bytes[self.pos] == b'.'
            && bytes.get(self.pos + 1).is_some_and(u8::is_ascii_digit)
        {
            self.pos += 1;
            while self.pos < bytes.len() && bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            return self.tok(TokenKind::Float, start);
        }

        self.tok(TokenKind::Int, start)
    }

    fn lex_string(&mut self, start: usize) -> Token {
        self.pos += 1; // opening quote
        loop {
            match self.peek() {
                None => {
                    return self.tok(TokenKind::Error(LexError::UnterminatedString), start);
                }
                Some(b'"') => {
                    self.pos += 1;
                    return self.tok(TokenKind::Str, start);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    if self.pos < self.source.len() {
                        let ch = self.source[self.pos..].chars().next().expect("non-empty");
                        self.pos += ch.len_utf8();
                    }
                }
                Some(b) if b < 0x80 => {
                    self.pos += 1;
                }
                Some(_) => {
                    let ch = self.source[self.pos..].chars().next().expect("non-empty");
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    fn lex_line_comment(&mut self, start: usize) -> Token {
        self.pos += 1; // second /
        let bytes = self.source.as_bytes();
        while self.pos < bytes.len() && bytes[self.pos] != b'\n' {
            self.pos += 1;
        }
        self.tok(TokenKind::LineComment, start)
    }

    fn lex_block_comment(&mut self, start: usize) -> Token {
        self.pos += 1; // opening *
        let mut depth: u32 = 1;

        while depth > 0 {
            match self.peek() {
                None => {
                    return self.tok(TokenKind::Error(LexError::UnterminatedBlockComment), start);
                }
                Some(b'/') => {
                    self.pos += 1;
                    if self.peek() == Some(b'*') {
                        self.pos += 1;
                        depth += 1;
                    }
                }
                Some(b'*') => {
                    self.pos += 1;
                    if self.peek() == Some(b'/') {
                        self.pos += 1;
                        depth -= 1;
                    }
                }
                Some(_) => {
                    self.pos += 1;
                }
            }
        }

        self.tok(TokenKind::BlockComment, start)
    }
}

impl<'src> Iterator for Lexer<'src> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        let tok = self.next_token();
        if tok.kind == TokenKind::Eof {
            None
        } else {
            Some(tok)
        }
    }
}

/// Returns `true` if `ident` violates Hird canonical naming.
///
/// Rules checked:
/// - Identifiers starting with a lowercase letter must be
///   `snake_case` (no uppercase bytes).
/// - Identifiers starting with an uppercase letter must be
///   `PascalCase` (no underscores).
/// - Leading underscores are stripped; the rule is chosen by the
///   first non-underscore letter. All-underscore identifiers
///   (e.g. `_`, `__`) are always valid.
fn has_naming_violation(ident: &str) -> bool {
    let bytes = ident.as_bytes();
    debug_assert!(!bytes.is_empty(), "identifiers are never empty");

    let rest = match bytes.iter().position(|&b| b != b'_') {
        Some(i) => &bytes[i..],
        None => return false,
    };

    let first = rest[0];

    if first.is_ascii_lowercase() {
        rest.iter().any(|b| b.is_ascii_uppercase())
    } else if first.is_ascii_uppercase() {
        rest.contains(&b'_')
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::vec;
    use alloc::vec::Vec;

    use super::Lexer;
    use crate::TokenKind::{self, *};
    use crate::{LexError, Token};

    fn kinds(source: &str) -> Vec<TokenKind> {
        Lexer::new(source, 0).map(|t| t.kind).collect()
    }

    fn tokens(source: &str) -> Vec<Token> {
        Lexer::new(source, 0).collect()
    }

    // -- empty / whitespace ------------------------------------------------

    #[test]
    fn empty_input() {
        assert_eq!(kinds(""), vec![]);
    }

    #[test]
    fn whitespace_only() {
        assert_eq!(kinds("  \t\n  "), vec![]);
    }

    // -- keywords ----------------------------------------------------------

    #[test]
    fn all_keywords() {
        let kws = [
            ("let", Let),
            ("fn", Fn),
            ("match", Match),
            ("type", Type),
            ("actor", Actor),
            ("supervisor", Supervisor),
            ("effect", Effect),
            ("tool", Tool),
            ("handle", Handle),
            ("spawn", Spawn),
            ("send", Send),
            ("request", Request),
            ("use", Use),
            ("module", Module),
            ("pub", Pub),
            ("extern", Extern),
            ("if", If),
            ("then", Then),
            ("else", Else),
        ];
        for (src, expected) in kws {
            assert_eq!(kinds(src), vec![expected], "keyword: {src}");
        }
    }

    #[test]
    fn keyword_prefix_is_ident() {
        assert_eq!(kinds("letter"), vec![Ident]);
        assert_eq!(kinds("fns"), vec![Ident]);
        assert_eq!(kinds("matching"), vec![Ident]);
    }

    #[test]
    fn keyword_is_keyword() {
        assert!(Let.is_keyword());
        assert!(Else.is_keyword());
        assert!(!Ident.is_keyword());
        assert!(!Plus.is_keyword());
    }

    // -- identifiers -------------------------------------------------------

    #[test]
    fn valid_snake_case() {
        assert_eq!(kinds("foo"), vec![Ident]);
        assert_eq!(kinds("foo_bar"), vec![Ident]);
        assert_eq!(kinds("x"), vec![Ident]);
        assert_eq!(kinds("a1"), vec![Ident]);
        assert_eq!(kinds("_foo"), vec![Ident]);
        assert_eq!(kinds("_"), vec![Ident]);
    }

    #[test]
    fn valid_pascal_case() {
        assert_eq!(kinds("Foo"), vec![Ident]);
        assert_eq!(kinds("FooBar"), vec![Ident]);
        assert_eq!(kinds("Config2"), vec![Ident]);
    }

    #[test]
    fn naming_violation_camel_case() {
        let bad = Error(LexError::NonCanonicalName);
        assert_eq!(kinds("fooBar"), vec![bad]);
        assert_eq!(kinds("camelCase"), vec![bad]);
        assert_eq!(kinds("getX"), vec![bad]);
    }

    #[test]
    fn naming_violation_pascal_with_underscore() {
        let bad = Error(LexError::NonCanonicalName);
        assert_eq!(kinds("Foo_Bar"), vec![bad]);
        assert_eq!(kinds("FOO_BAR"), vec![bad]);
    }

    #[test]
    fn naming_violation_with_leading_underscore() {
        let bad = Error(LexError::NonCanonicalName);
        assert_eq!(kinds("_fooBar"), vec![bad]);
        assert_eq!(kinds("_Foo_Bar"), vec![bad]);
    }

    #[test]
    fn naming_violation_with_multiple_leading_underscores() {
        let bad = Error(LexError::NonCanonicalName);
        assert_eq!(kinds("__camelCase"), vec![bad]);
        assert_eq!(kinds("__Foo_Bar"), vec![bad]);
        assert_eq!(kinds("___getX"), vec![bad]);
    }

    #[test]
    fn all_underscores_are_valid() {
        assert_eq!(kinds("__"), vec![Ident]);
        assert_eq!(kinds("___"), vec![Ident]);
    }

    // -- integer literals --------------------------------------------------

    #[test]
    fn integer_literals() {
        assert_eq!(kinds("0"), vec![Int]);
        assert_eq!(kinds("42"), vec![Int]);
        assert_eq!(kinds("123456"), vec![Int]);
    }

    // -- float literals ----------------------------------------------------

    #[test]
    fn float_literals() {
        assert_eq!(kinds("0.0"), vec![Float]);
        assert_eq!(kinds("3.14"), vec![Float]);
        assert_eq!(kinds("1.5"), vec![Float]);
    }

    #[test]
    fn dot_without_trailing_digit_is_not_float() {
        assert_eq!(kinds("42."), vec![Int, Dot]);
        assert_eq!(kinds("42.x"), vec![Int, Dot, Ident]);
    }

    // -- string literals ---------------------------------------------------

    #[test]
    fn string_literals() {
        assert_eq!(kinds(r#""""#), vec![Str]);
        assert_eq!(kinds(r#""hello""#), vec![Str]);
        assert_eq!(kinds(r#""with \"escape\"""#), vec![Str]);
        assert_eq!(kinds(r#""line\nbreak""#), vec![Str]);
    }

    #[test]
    fn unterminated_string() {
        assert_eq!(kinds(r#""oops"#), vec![Error(LexError::UnterminatedString)]);
    }

    #[test]
    fn string_with_escaped_backslash() {
        assert_eq!(kinds(r#""\\""#), vec![Str]);
    }

    // -- single-character operators ----------------------------------------

    #[test]
    fn single_char_operators() {
        assert_eq!(kinds("+"), vec![Plus]);
        assert_eq!(kinds("-"), vec![Minus]);
        assert_eq!(kinds("*"), vec![Star]);
        assert_eq!(kinds("/"), vec![Slash]);
        assert_eq!(kinds("<"), vec![Lt]);
        assert_eq!(kinds(">"), vec![Gt]);
        assert_eq!(kinds("="), vec![Eq]);
        assert_eq!(kinds("|"), vec![Pipe]);
        assert_eq!(kinds("!"), vec![Bang]);
        assert_eq!(kinds("."), vec![Dot]);
        assert_eq!(kinds(":"), vec![Colon]);
    }

    // -- multi-character operators -----------------------------------------

    #[test]
    fn multi_char_operators() {
        assert_eq!(kinds("->"), vec![Arrow]);
        assert_eq!(kinds("=>"), vec![FatArrow]);
        assert_eq!(kinds("<="), vec![Le]);
        assert_eq!(kinds(">="), vec![Ge]);
        assert_eq!(kinds("=="), vec![EqEq]);
        assert_eq!(kinds("!="), vec![BangEq]);
        assert_eq!(kinds("::"), vec![ColonColon]);
    }

    #[test]
    fn lambda_ascii() {
        assert_eq!(kinds("\\"), vec![Lambda]);
    }

    // -- Unicode normalisation ---------------------------------------------

    #[test]
    fn unicode_arrow_normalisation() {
        assert_eq!(kinds("\u{2192}"), vec![Arrow]);
        assert_eq!(kinds("->"), kinds("\u{2192}"));
    }

    #[test]
    fn unicode_fat_arrow_normalisation() {
        assert_eq!(kinds("\u{21d2}"), vec![FatArrow]);
        assert_eq!(kinds("=>"), kinds("\u{21d2}"));
    }

    #[test]
    fn unicode_lambda_normalisation() {
        assert_eq!(kinds("\u{03bb}"), vec![Lambda]);
        assert_eq!(kinds("\\"), kinds("\u{03bb}"));
    }

    #[test]
    fn unicode_normalisation_in_expression() {
        let ascii = kinds("\\x -> x");
        let unicode = kinds("\u{03bb}x \u{2192} x");
        assert_eq!(ascii, unicode);
        assert_eq!(ascii, vec![Lambda, Ident, Arrow, Ident]);
    }

    // -- delimiters --------------------------------------------------------

    #[test]
    fn delimiters() {
        assert_eq!(kinds("("), vec![LParen]);
        assert_eq!(kinds(")"), vec![RParen]);
        assert_eq!(kinds("{"), vec![LBrace]);
        assert_eq!(kinds("}"), vec![RBrace]);
        assert_eq!(kinds("["), vec![LBracket]);
        assert_eq!(kinds("]"), vec![RBracket]);
        assert_eq!(kinds(","), vec![Comma]);
        assert_eq!(kinds(";"), vec![Semicolon]);
    }

    // -- comments ----------------------------------------------------------

    #[test]
    fn line_comment() {
        assert_eq!(kinds("// hello"), vec![LineComment]);
        assert_eq!(kinds("// hello\n42"), vec![LineComment, Int]);
    }

    #[test]
    fn block_comment() {
        assert_eq!(kinds("/* hello */"), vec![BlockComment]);
    }

    #[test]
    fn nested_block_comment() {
        assert_eq!(kinds("/* outer /* inner */ end */"), vec![BlockComment]);
    }

    #[test]
    fn deeply_nested_block_comment() {
        assert_eq!(kinds("/* a /* b /* c */ d */ e */"), vec![BlockComment]);
    }

    #[test]
    fn unterminated_block_comment() {
        assert_eq!(
            kinds("/* oops"),
            vec![Error(LexError::UnterminatedBlockComment)]
        );
    }

    #[test]
    fn comment_classification() {
        assert!(LineComment.is_comment());
        assert!(BlockComment.is_comment());
        assert!(!Ident.is_comment());
    }

    // -- error recovery ----------------------------------------------------

    #[test]
    fn unexpected_char() {
        assert_eq!(kinds("@"), vec![Error(LexError::UnexpectedChar)]);
    }

    #[test]
    fn error_recovery_continues() {
        assert_eq!(
            kinds("42 @ 7"),
            vec![Int, Error(LexError::UnexpectedChar), Int]
        );
    }

    #[test]
    fn multiple_errors_in_sequence() {
        assert_eq!(
            kinds("@#"),
            vec![
                Error(LexError::UnexpectedChar),
                Error(LexError::UnexpectedChar),
            ]
        );
    }

    #[test]
    fn error_is_error() {
        assert!(Error(LexError::UnexpectedChar).is_error());
        assert!(!Ident.is_error());
    }

    // -- spans -------------------------------------------------------------

    #[test]
    fn span_tracking() {
        let toks = tokens("let x = 42");
        assert_eq!(toks.len(), 4);

        assert_eq!(toks[0].kind, Let);
        assert_eq!(toks[0].span.start, 0);
        assert_eq!(toks[0].span.end, 3);

        assert_eq!(toks[1].kind, Ident);
        assert_eq!(toks[1].span.start, 4);
        assert_eq!(toks[1].span.end, 5);

        assert_eq!(toks[2].kind, Eq);
        assert_eq!(toks[2].span.start, 6);
        assert_eq!(toks[2].span.end, 7);

        assert_eq!(toks[3].kind, Int);
        assert_eq!(toks[3].span.start, 8);
        assert_eq!(toks[3].span.end, 10);
    }

    #[test]
    fn span_text_extraction() {
        let src = "let foo = 42";
        let toks = tokens(src);
        assert_eq!(toks[1].span.text(src), "foo");
        assert_eq!(toks[3].span.text(src), "42");
    }

    #[test]
    fn unicode_arrow_span_width() {
        let ascii_toks = tokens("->");
        assert_eq!(ascii_toks[0].span.len(), 2);

        let unicode_toks = tokens("\u{2192}");
        assert_eq!(unicode_toks[0].span.len(), 3);
    }

    #[test]
    fn source_id_propagated() {
        let mut lex = Lexer::new("x", 99);
        let tok = lex.next_token();
        assert_eq!(tok.span.source_id, 99);
    }

    // -- Eof behaviour -----------------------------------------------------

    #[test]
    fn eof_idempotent() {
        let mut lex = Lexer::new("", 0);
        assert_eq!(lex.next_token().kind, Eof);
        assert_eq!(lex.next_token().kind, Eof);
    }

    #[test]
    fn eof_span_at_end() {
        let mut lex = Lexer::new("ab", 0);
        let _ = lex.next_token(); // Ident "ab"
        let eof = lex.next_token();
        assert_eq!(eof.kind, Eof);
        assert!(eof.span.is_empty());
        assert_eq!(eof.span.start, 2);
    }

    // -- realistic snippet -------------------------------------------------

    #[test]
    fn function_declaration() {
        let src = "fn add(x: Int, y: Int) \u{2192} Int ! {} {\n  x + y\n}";
        assert_eq!(
            kinds(src),
            vec![
                Fn, Ident, LParen, Ident, Colon, Ident, Comma, Ident, Colon, Ident, RParen, Arrow,
                Ident, Bang, LBrace, RBrace, LBrace, Ident, Plus, Ident, RBrace,
            ]
        );
    }
}

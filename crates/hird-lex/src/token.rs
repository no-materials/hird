// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Token, span, and error types for the Hird lexer.

/// Byte range within a source file.
///
/// Offsets are byte positions in the UTF-8 source. Every [`Token`]
/// carries a `Span` so later passes can produce accurate diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Byte offset of the first byte (inclusive).
    pub start: u32,
    /// Byte offset past the last byte (exclusive).
    pub end: u32,
    /// Identifies the source file this span belongs to.
    pub source_id: u32,
}

impl Span {
    /// Creates a span from byte offsets and a source identifier.
    #[must_use]
    pub const fn new(start: u32, end: u32, source_id: u32) -> Self {
        debug_assert!(end >= start, "span end precedes start");
        Self {
            start,
            end,
            source_id,
        }
    }

    /// Returns the byte length of this span.
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.end - self.start
    }

    /// Returns `true` if the span covers zero bytes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Extracts the spanned slice from `source`.
    #[must_use]
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start as usize..self.end as usize]
    }
}

/// A lexed token carrying its classification and source location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// What kind of token this is.
    pub kind: TokenKind,
    /// Where in the source this token appears.
    pub span: Span,
}

/// Classifies a [`Token`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // --- Keywords ---
    /// `let`
    Let,
    /// `fn`
    Fn,
    /// `match`
    Match,
    /// `type`
    Type,
    /// `actor`
    Actor,
    /// `supervisor`
    Supervisor,
    /// `effect`
    Effect,
    /// `tool`
    Tool,
    /// `handle`
    Handle,
    /// `spawn`
    Spawn,
    /// `send`
    Send,
    /// `request`
    Request,
    /// `use`
    Use,
    /// `module`
    Module,
    /// `pub`
    Pub,
    /// `opaque`
    Opaque,
    /// `extern`
    Extern,
    /// `if`
    If,
    /// `then`
    Then,
    /// `else`
    Else,
    /// `in`
    In,

    // --- Identifiers and literals ---
    /// An identifier that passed keyword lookup and naming checks.
    Ident,
    /// Integer literal (e.g. `42`).
    Int,
    /// Floating-point literal (e.g. `3.14`).
    Float,
    /// String literal including its quotes (e.g. `"hello"`).
    Str,

    // --- Operators ---
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `==`
    EqEq,
    /// `!=`
    BangEq,
    /// `=`
    Eq,
    /// `\u{2192}` (also written `->`)
    Arrow,
    /// `\u{21d2}` (also written `=>`)
    FatArrow,
    /// `\u{03bb}` (also written `\`)
    Lambda,
    /// `|`
    Pipe,
    /// `\u{2227}` (also written `&&`)
    AmpAmp,
    /// `\u{2228}` (also written `||`)
    PipePipe,
    /// `!`
    Bang,
    /// `.`
    Dot,
    /// `:`
    Colon,
    /// `::`
    ColonColon,

    // --- Delimiters ---
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `,`
    Comma,
    /// `;`
    Semicolon,

    // --- Trivia ---
    /// Line comment (`// ...`).
    LineComment,
    /// Block comment (`/* ... */`), may be nested.
    BlockComment,

    // --- Special ---
    /// End of input.
    Eof,
    /// Lexer error with a diagnostic classification.
    Error(LexError),
}

/// Describes a lexing error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexError {
    /// String literal missing its closing `"`.
    UnterminatedString,
    /// Block comment missing its closing `*/`.
    UnterminatedBlockComment,
    /// Byte sequence not recognised as the start of any token.
    UnexpectedChar,
    /// Identifier violates canonical naming conventions.
    NonCanonicalName,
}

impl TokenKind {
    /// Returns `true` for keyword token kinds.
    #[must_use]
    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            Self::Let
                | Self::Fn
                | Self::Match
                | Self::Type
                | Self::Actor
                | Self::Supervisor
                | Self::Effect
                | Self::Tool
                | Self::Handle
                | Self::Spawn
                | Self::Send
                | Self::Request
                | Self::Use
                | Self::Module
                | Self::Pub
                | Self::Opaque
                | Self::Extern
                | Self::If
                | Self::Then
                | Self::Else
                | Self::In
        )
    }

    /// Returns `true` for error token kinds.
    #[must_use]
    pub fn is_error(self) -> bool {
        matches!(self, Self::Error(_))
    }

    /// Returns `true` for comment token kinds.
    #[must_use]
    pub fn is_comment(self) -> bool {
        matches!(self, Self::LineComment | Self::BlockComment)
    }
}

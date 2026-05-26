// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Recursive descent parser skeleton.
//!
//! Consumes a token stream from [`hird_lex::Lexer`], synthesises
//! whitespace tokens for gaps, and builds a cstree green tree.

use alloc::vec::Vec;

use cstree::build::{Checkpoint, GreenNodeBuilder};
use cstree::green::GreenNode;
use hird_lex::{Lexer, Token};

use crate::diagnostic::ParseDiagnostic;
use crate::syntax_kind::SyntaxKind;

/// Result of parsing a source file.
#[derive(Debug)]
pub struct ParseResult {
    /// Root green node of the CST.
    green: GreenNode,
    /// Diagnostics emitted during parsing.
    diagnostics: Vec<ParseDiagnostic>,
}

impl ParseResult {
    /// Returns the root green node.
    #[must_use]
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// Returns diagnostics emitted during parsing.
    #[must_use]
    pub fn diagnostics(&self) -> &[ParseDiagnostic] {
        &self.diagnostics
    }

    /// Returns `true` if parsing produced no errors.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Parse `source` into a CST.
///
/// The returned [`ParseResult`] contains the green tree root and any
/// diagnostics. Use [`cstree::syntax::SyntaxNode::new_root`] with the
/// green node to get a traversable syntax tree.
pub fn parse(source: &str, source_id: u32) -> ParseResult {
    let tokens: Vec<Token> = Lexer::new(source, source_id).collect();
    let mut parser = Parser::new(source, &tokens);
    parser.parse_source_file();
    let (green, _cache) = parser.builder.finish();
    ParseResult {
        green,
        diagnostics: parser.diagnostics,
    }
}

/// Internal parser state.
struct Parser<'src, 'tok> {
    source: &'src str,
    tokens: &'tok [Token],
    pos: usize,
    /// Byte offset of the end of the last emitted token (for whitespace gaps).
    prev_end: u32,
    builder: GreenNodeBuilder<'static, 'static, SyntaxKind>,
    diagnostics: Vec<ParseDiagnostic>,
}

impl<'src, 'tok> Parser<'src, 'tok> {
    fn new(source: &'src str, tokens: &'tok [Token]) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            prev_end: 0,
            builder: GreenNodeBuilder::new(),
            diagnostics: Vec::new(),
        }
    }

    // === Tree construction helpers ===

    /// Starts a new CST node.
    fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(kind);
    }

    /// Finishes the current CST node.
    fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    /// Records a checkpoint for retroactive node wrapping.
    #[expect(dead_code, reason = "infrastructure for grammar productions")]
    fn checkpoint(&mut self) -> Checkpoint {
        self.builder.checkpoint()
    }

    /// Wraps tokens emitted since `checkpoint` in a new node.
    #[expect(dead_code, reason = "infrastructure for grammar productions")]
    fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(checkpoint, kind);
    }

    // === Token access ===

    /// Returns the current token kind, or `EOF` if past the end.
    fn current(&self) -> SyntaxKind {
        self.tokens
            .get(self.pos)
            .map_or(SyntaxKind::EOF, |t| SyntaxKind::from(t.kind))
    }

    /// Returns `true` if the current token matches `kind`.
    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    /// Returns `true` if all tokens have been consumed.
    fn at_eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    // === Token consumption ===

    /// Emits whitespace for any gap before the current token, then
    /// emits the current token and advances.
    fn bump(&mut self) {
        if self.at_eof() {
            return;
        }

        let tok = self.tokens[self.pos];
        self.emit_whitespace_before(tok.span.start);
        let text = tok.span.text(self.source);
        self.builder.token(SyntaxKind::from(tok.kind), text);
        self.prev_end = tok.span.end;
        self.pos += 1;
    }

    /// Advances past the current token if it matches `kind`.
    /// Returns `true` if matched.
    #[expect(dead_code, reason = "infrastructure for grammar productions")]
    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Emits a WHITESPACE token for the gap between `prev_end` and
    /// `next_start`, if any.
    fn emit_whitespace_before(&mut self, next_start: u32) {
        if next_start > self.prev_end {
            let ws = &self.source[self.prev_end as usize..next_start as usize];
            self.builder.token(SyntaxKind::WHITESPACE, ws);
            self.prev_end = next_start;
        }
    }

    /// Emits trailing whitespace after the last token to the end of source.
    fn emit_trailing_whitespace(&mut self) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "source length checked at Lexer::new"
        )]
        let source_len = self.source.len() as u32;
        self.emit_whitespace_before(source_len);
    }

    // === Parsing ===

    /// Top-level entry point: wraps everything in a `SOURCE_FILE` node.
    fn parse_source_file(&mut self) {
        self.start_node(SyntaxKind::SOURCE_FILE);

        // Emit every token flat under the root until grammar
        // productions are implemented.
        while !self.at_eof() {
            self.bump();
        }

        self.emit_trailing_whitespace();
        self.finish_node();
    }
}

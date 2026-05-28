// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Recursive descent parser.
//!
//! Consumes a token stream from [`hird_lex::Lexer`], synthesises
//! whitespace tokens for gaps, and builds a cstree green tree.

use alloc::vec::Vec;

use cstree::build::{Checkpoint, GreenNodeBuilder};
use cstree::green::GreenNode;
use hird_lex::{Lexer, Span, Token};

use crate::diagnostic::{DiagnosticCode, ParseDiagnostic};
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
    let mut parser = Parser::new(source, source_id, &tokens);
    parser.parse_source_file();
    let (green, _cache) = parser.builder.finish();
    ParseResult {
        green,
        diagnostics: parser.diagnostics,
    }
}

const MAX_NESTING: u32 = 256;

struct Parser<'src, 'tok> {
    source: &'src str,
    source_id: u32,
    tokens: &'tok [Token],
    pos: usize,
    prev_end: u32,
    depth: u32,
    builder: GreenNodeBuilder<'static, 'static, SyntaxKind>,
    diagnostics: Vec<ParseDiagnostic>,
}

impl<'src, 'tok> Parser<'src, 'tok> {
    fn new(source: &'src str, source_id: u32, tokens: &'tok [Token]) -> Self {
        Self {
            source,
            source_id,
            tokens,
            pos: 0,
            prev_end: 0,
            depth: 0,
            builder: GreenNodeBuilder::new(),
            diagnostics: Vec::new(),
        }
    }

    // ── trivia helpers ──────────────────────────────────────────

    fn is_trivia(kind: SyntaxKind) -> bool {
        matches!(kind, SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT)
    }

    /// Kind of the next non-trivia token (EOF if none remain).
    fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    /// Look-ahead past trivia. `nth(0)` == `current()`.
    fn nth(&self, n: usize) -> SyntaxKind {
        let mut pos = self.pos;
        let mut seen = 0;
        while pos < self.tokens.len() {
            let kind = SyntaxKind::from(self.tokens[pos].kind);
            if !Self::is_trivia(kind) {
                if seen == n {
                    return kind;
                }
                seen += 1;
            }
            pos += 1;
        }
        SyntaxKind::EOF
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    fn at_contextual(&self, text: &str) -> bool {
        let mut pos = self.pos;
        while pos < self.tokens.len() {
            let kind = SyntaxKind::from(self.tokens[pos].kind);
            if !Self::is_trivia(kind) {
                return kind == SyntaxKind::IDENT
                    && self.tokens[pos].span.text(self.source) == text;
            }
            pos += 1;
        }
        false
    }

    fn current_span(&self) -> Span {
        let mut pos = self.pos;
        while pos < self.tokens.len() {
            let kind = SyntaxKind::from(self.tokens[pos].kind);
            if !Self::is_trivia(kind) {
                return self.tokens[pos].span;
            }
            pos += 1;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "source length checked at Lexer::new"
        )]
        let end = self.source.len() as u32;
        Span::new(end, end, self.source_id)
    }

    // ── token consumption ───────────────────────────────────────

    fn bump_raw(&mut self) {
        if self.pos >= self.tokens.len() {
            return;
        }
        let tok = self.tokens[self.pos];
        self.emit_whitespace_before(tok.span.start);
        let text = tok.span.text(self.source);
        self.builder.token(SyntaxKind::from(tok.kind), text);
        self.prev_end = tok.span.end;
        self.pos += 1;
    }

    fn eat_trivia(&mut self) {
        while self.pos < self.tokens.len() {
            let kind = SyntaxKind::from(self.tokens[self.pos].kind);
            if !Self::is_trivia(kind) {
                break;
            }
            self.bump_raw();
        }
    }

    /// Emit leading trivia then the next significant token.
    fn bump(&mut self) {
        self.eat_trivia();
        self.bump_raw();
    }

    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            self.diagnostics.push(ParseDiagnostic {
                code: DiagnosticCode::P0001,
                span: self.current_span(),
                message: expected_msg(kind),
            });
            false
        }
    }

    fn error_bump(&mut self, message: &'static str) {
        let span = self.current_span();
        if self.current() == SyntaxKind::EOF {
            self.diagnostics.push(ParseDiagnostic {
                code: DiagnosticCode::P0002,
                span,
                message,
            });
            return;
        }
        self.start_node(SyntaxKind::ERROR);
        self.diagnostics.push(ParseDiagnostic {
            code: DiagnosticCode::P0002,
            span,
            message,
        });
        self.bump();
        self.finish_node();
    }

    fn too_deep(&mut self) -> bool {
        if self.depth >= MAX_NESTING {
            self.diagnostics.push(ParseDiagnostic {
                code: DiagnosticCode::P0004,
                span: self.current_span(),
                message: "nesting depth limit reached",
            });
            return true;
        }
        false
    }

    // ── tree construction ───────────────────────────────────────

    fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(kind);
    }

    fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    fn checkpoint(&mut self) -> Checkpoint {
        self.builder.checkpoint()
    }

    fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(checkpoint, kind);
    }

    // ── whitespace ──────────────────────────────────────────────

    fn emit_whitespace_before(&mut self, next_start: u32) {
        if next_start > self.prev_end {
            let ws = &self.source[self.prev_end as usize..next_start as usize];
            self.builder.token(SyntaxKind::WHITESPACE, ws);
            self.prev_end = next_start;
        }
    }

    fn emit_trailing_whitespace(&mut self) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "source length checked at Lexer::new"
        )]
        let source_len = self.source.len() as u32;
        self.emit_whitespace_before(source_len);
    }

    fn drain_remaining(&mut self) {
        while self.pos < self.tokens.len() {
            self.bump_raw();
        }
        self.emit_trailing_whitespace();
    }

    // ── source file ─────────────────────────────────────────────

    fn parse_source_file(&mut self) {
        self.start_node(SyntaxKind::SOURCE_FILE);

        if self.current() == SyntaxKind::MODULE_KW {
            self.parse_module_decl();
        }

        while self.current() != SyntaxKind::EOF {
            self.parse_top_item();
        }

        self.drain_remaining();
        self.finish_node();
    }

    fn parse_top_item(&mut self) {
        match self.current() {
            SyntaxKind::USE_KW => self.parse_use_decl(),
            SyntaxKind::FN_KW => self.parse_fn_decl(),
            SyntaxKind::TYPE_KW => self.parse_type_decl(),
            SyntaxKind::ACTOR_KW => self.parse_actor_decl(),
            SyntaxKind::SUPERVISOR_KW => self.parse_supervisor_decl(),
            SyntaxKind::EFFECT_KW => self.parse_effect_decl(),
            SyntaxKind::TOOL_KW => self.parse_tool_decl(),
            SyntaxKind::EXTERN_KW => self.parse_extern_decl(),
            SyntaxKind::PUB_KW => match self.nth(1) {
                SyntaxKind::FN_KW => self.parse_fn_decl(),
                SyntaxKind::TYPE_KW => self.parse_type_decl(),
                SyntaxKind::ACTOR_KW => self.parse_actor_decl(),
                SyntaxKind::SUPERVISOR_KW => self.parse_supervisor_decl(),
                SyntaxKind::EFFECT_KW => self.parse_effect_decl(),
                SyntaxKind::TOOL_KW => self.parse_tool_decl(),
                _ => self.error_bump("expected declaration after `pub`"),
            },
            _ => self.error_bump("expected declaration"),
        }
    }

    fn parse_visibility(&mut self) {
        if self.at(SyntaxKind::PUB_KW) {
            self.start_node(SyntaxKind::VISIBILITY);
            self.bump();
            self.finish_node();
        }
    }

    // ── declarations ────────────────────────────────────────────

    fn parse_module_decl(&mut self) {
        self.start_node(SyntaxKind::MODULE_DECL);
        self.expect(SyntaxKind::MODULE_KW);
        self.expect(SyntaxKind::IDENT);
        self.finish_node();
    }

    fn parse_use_decl(&mut self) {
        self.start_node(SyntaxKind::USE_DECL);
        self.expect(SyntaxKind::USE_KW);
        self.parse_path();
        if self.at_contextual("as") {
            self.bump();
            self.expect(SyntaxKind::IDENT);
        }
        self.finish_node();
    }

    fn parse_path(&mut self) {
        self.start_node(SyntaxKind::PATH);
        self.expect(SyntaxKind::IDENT);
        while self.eat(SyntaxKind::COLON_COLON) {
            self.expect(SyntaxKind::IDENT);
        }
        self.finish_node();
    }

    fn parse_fn_decl(&mut self) {
        self.start_node(SyntaxKind::FN_DECL);
        self.parse_visibility();
        self.expect(SyntaxKind::FN_KW);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::L_PAREN);
        if !self.at(SyntaxKind::R_PAREN) {
            self.parse_param_list();
        }
        self.expect(SyntaxKind::R_PAREN);
        if self.at(SyntaxKind::ARROW) {
            self.parse_return_type();
        }
        if self.at(SyntaxKind::BANG) {
            self.parse_effect_ann();
        }
        self.expect(SyntaxKind::EQ);
        self.parse_expr();
        self.finish_node();
    }

    fn parse_param_list(&mut self) {
        self.start_node(SyntaxKind::PARAM_LIST);
        self.parse_param();
        while self.eat(SyntaxKind::COMMA) {
            if self.at(SyntaxKind::R_PAREN) {
                break;
            }
            self.parse_param();
        }
        self.finish_node();
    }

    fn parse_param(&mut self) {
        self.start_node(SyntaxKind::PARAM);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::COLON);
        self.parse_type_expr();
        self.finish_node();
    }

    fn parse_return_type(&mut self) {
        self.start_node(SyntaxKind::RETURN_TYPE);
        self.expect(SyntaxKind::ARROW);
        self.parse_type_expr();
        self.finish_node();
    }

    fn parse_effect_ann(&mut self) {
        self.start_node(SyntaxKind::EFFECT_ANN);
        self.expect(SyntaxKind::BANG);
        self.expect(SyntaxKind::L_BRACE);
        if !self.at(SyntaxKind::R_BRACE) {
            self.parse_type_expr();
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_BRACE) {
                    break;
                }
                self.parse_type_expr();
            }
        }
        self.expect(SyntaxKind::R_BRACE);
        self.finish_node();
    }

    fn parse_type_decl(&mut self) {
        self.start_node(SyntaxKind::TYPE_DECL);
        self.parse_visibility();
        self.expect(SyntaxKind::TYPE_KW);
        self.expect(SyntaxKind::IDENT);
        if self.at(SyntaxKind::LT) {
            self.parse_type_params();
        }
        self.expect(SyntaxKind::EQ);
        self.parse_constructor();
        while self.eat(SyntaxKind::PIPE) {
            self.parse_constructor();
        }
        self.finish_node();
    }

    fn parse_type_params(&mut self) {
        self.start_node(SyntaxKind::TYPE_PARAMS);
        self.expect(SyntaxKind::LT);
        self.expect(SyntaxKind::IDENT);
        while self.eat(SyntaxKind::COMMA) {
            if self.at(SyntaxKind::GT) {
                break;
            }
            self.expect(SyntaxKind::IDENT);
        }
        self.expect(SyntaxKind::GT);
        self.finish_node();
    }

    fn parse_constructor(&mut self) {
        self.start_node(SyntaxKind::CONSTRUCTOR);
        self.expect(SyntaxKind::IDENT);
        if self.eat(SyntaxKind::L_PAREN) {
            if !self.at(SyntaxKind::R_PAREN) {
                self.parse_field_list();
            }
            self.expect(SyntaxKind::R_PAREN);
        }
        self.finish_node();
    }

    fn parse_field_list(&mut self) {
        self.start_node(SyntaxKind::FIELD_LIST);
        self.parse_type_expr();
        while self.eat(SyntaxKind::COMMA) {
            if self.at(SyntaxKind::R_PAREN) {
                break;
            }
            self.parse_type_expr();
        }
        self.finish_node();
    }

    fn parse_actor_decl(&mut self) {
        self.start_node(SyntaxKind::ACTOR_DECL);
        self.parse_visibility();
        self.expect(SyntaxKind::ACTOR_KW);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::L_BRACE);
        if !self.at(SyntaxKind::R_BRACE) {
            self.parse_actor_field();
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_BRACE) {
                    break;
                }
                self.parse_actor_field();
            }
        }
        self.expect(SyntaxKind::R_BRACE);
        self.finish_node();
    }

    fn parse_actor_field(&mut self) {
        self.start_node(SyntaxKind::ACTOR_FIELD);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::COLON);
        self.parse_expr();
        self.finish_node();
    }

    fn parse_supervisor_decl(&mut self) {
        self.start_node(SyntaxKind::SUPERVISOR_DECL);
        self.parse_visibility();
        self.expect(SyntaxKind::SUPERVISOR_KW);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::L_BRACE);
        if !self.at(SyntaxKind::R_BRACE) {
            self.parse_supervisor_field();
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_BRACE) {
                    break;
                }
                self.parse_supervisor_field();
            }
        }
        self.expect(SyntaxKind::R_BRACE);
        self.finish_node();
    }

    fn parse_supervisor_field(&mut self) {
        self.start_node(SyntaxKind::SUPERVISOR_FIELD);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::COLON);
        self.parse_expr();
        self.finish_node();
    }

    fn parse_effect_decl(&mut self) {
        self.start_node(SyntaxKind::EFFECT_DECL);
        self.parse_visibility();
        self.expect(SyntaxKind::EFFECT_KW);
        self.expect(SyntaxKind::IDENT);
        if self.at(SyntaxKind::LT) {
            self.parse_type_params();
        }
        self.finish_node();
    }

    fn parse_tool_decl(&mut self) {
        self.start_node(SyntaxKind::TOOL_DECL);
        self.parse_visibility();
        self.expect(SyntaxKind::TOOL_KW);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::COLON);
        self.parse_app_type();
        self.expect(SyntaxKind::ARROW);
        self.parse_type_expr();
        self.finish_node();
    }

    fn parse_extern_decl(&mut self) {
        self.start_node(SyntaxKind::EXTERN_DECL);
        self.expect(SyntaxKind::EXTERN_KW);
        self.expect(SyntaxKind::FN_KW);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::L_PAREN);
        if !self.at(SyntaxKind::R_PAREN) {
            self.parse_param_list();
        }
        self.expect(SyntaxKind::R_PAREN);
        if self.at(SyntaxKind::ARROW) {
            self.parse_return_type();
        }
        self.finish_node();
    }

    // ── type expressions ────────────────────────────────────────

    fn parse_type_expr(&mut self) {
        if self.too_deep() {
            return;
        }
        self.depth += 1;
        self.parse_fn_type();
        self.depth -= 1;
    }

    fn parse_fn_type(&mut self) {
        let cp = self.checkpoint();
        self.parse_app_type();
        if self.at(SyntaxKind::ARROW) {
            self.start_node_at(cp, SyntaxKind::FN_TYPE);
            while self.at(SyntaxKind::ARROW) {
                self.bump();
                self.parse_app_type();
                if self.at(SyntaxKind::BANG) {
                    self.parse_effect_ann();
                }
            }
            self.finish_node();
        }
    }

    fn parse_app_type(&mut self) {
        let cp = self.checkpoint();
        self.parse_atom_type();
        if self.at(SyntaxKind::LT) {
            self.start_node_at(cp, SyntaxKind::APP_TYPE);
            self.parse_type_args();
            self.finish_node();
        }
    }

    fn parse_atom_type(&mut self) {
        match self.current() {
            SyntaxKind::IDENT => {
                self.bump();
            }
            SyntaxKind::L_PAREN => {
                let cp = self.checkpoint();
                self.bump();
                if self.at(SyntaxKind::R_PAREN) {
                    self.start_node_at(cp, SyntaxKind::TUPLE_TYPE);
                    self.bump();
                    self.finish_node();
                    return;
                }
                self.parse_type_expr();
                if self.at(SyntaxKind::COMMA) {
                    self.start_node_at(cp, SyntaxKind::TUPLE_TYPE);
                    while self.eat(SyntaxKind::COMMA) {
                        if self.at(SyntaxKind::R_PAREN) {
                            break;
                        }
                        self.parse_type_expr();
                    }
                    self.expect(SyntaxKind::R_PAREN);
                    self.finish_node();
                } else {
                    self.start_node_at(cp, SyntaxKind::PAREN_TYPE);
                    self.expect(SyntaxKind::R_PAREN);
                    self.finish_node();
                }
            }
            _ => {
                self.error_bump("expected type");
            }
        }
    }

    fn parse_type_args(&mut self) {
        self.start_node(SyntaxKind::TYPE_ARGS);
        self.expect(SyntaxKind::LT);
        self.parse_type_expr();
        while self.eat(SyntaxKind::COMMA) {
            if self.at(SyntaxKind::GT) {
                break;
            }
            self.parse_type_expr();
        }
        self.expect(SyntaxKind::GT);
        self.finish_node();
    }

    // ── expressions ─────────────────────────────────────────────

    fn parse_expr(&mut self) {
        if self.too_deep() {
            return;
        }
        self.depth += 1;
        match self.current() {
            SyntaxKind::LET_KW => self.parse_let_expr(),
            SyntaxKind::LAMBDA => self.parse_lambda_expr(),
            SyntaxKind::IF_KW => self.parse_if_expr(),
            _ => self.parse_atom_expr(),
        }
        self.depth -= 1;
    }

    fn parse_let_expr(&mut self) {
        self.start_node(SyntaxKind::LET_EXPR);
        self.expect(SyntaxKind::LET_KW);
        self.expect(SyntaxKind::IDENT);
        if self.at(SyntaxKind::COLON) {
            self.start_node(SyntaxKind::TYPE_ANN);
            self.bump();
            self.parse_type_expr();
            self.finish_node();
        }
        self.expect(SyntaxKind::EQ);
        self.parse_expr();
        self.expect(SyntaxKind::IN_KW);
        self.parse_expr();
        self.finish_node();
    }

    fn parse_lambda_expr(&mut self) {
        self.start_node(SyntaxKind::LAMBDA_EXPR);
        self.expect(SyntaxKind::LAMBDA);
        self.expect(SyntaxKind::IDENT);
        while self.at(SyntaxKind::IDENT) {
            self.bump();
        }
        self.expect(SyntaxKind::ARROW);
        self.parse_expr();
        self.finish_node();
    }

    fn parse_if_expr(&mut self) {
        self.start_node(SyntaxKind::IF_EXPR);
        self.expect(SyntaxKind::IF_KW);
        self.parse_expr();
        self.expect(SyntaxKind::THEN_KW);
        self.parse_expr();
        self.expect(SyntaxKind::ELSE_KW);
        self.parse_expr();
        self.finish_node();
    }

    fn parse_atom_expr(&mut self) {
        match self.current() {
            SyntaxKind::IDENT | SyntaxKind::INT | SyntaxKind::FLOAT | SyntaxKind::STRING => {
                self.bump();
            }
            SyntaxKind::L_PAREN => {
                self.start_node(SyntaxKind::PAREN_EXPR);
                self.bump();
                self.parse_expr();
                self.expect(SyntaxKind::R_PAREN);
                self.finish_node();
            }
            _ => {
                self.error_bump("expected expression");
            }
        }
    }
}

fn expected_msg(kind: SyntaxKind) -> &'static str {
    match kind {
        SyntaxKind::IDENT => "expected identifier",
        SyntaxKind::EQ => "expected `=`",
        SyntaxKind::ARROW => "expected `\u{2192}`",
        SyntaxKind::COLON => "expected `:`",
        SyntaxKind::COLON_COLON => "expected `::`",
        SyntaxKind::L_PAREN => "expected `(`",
        SyntaxKind::R_PAREN => "expected `)`",
        SyntaxKind::L_BRACE => "expected `{`",
        SyntaxKind::R_BRACE => "expected `}`",
        SyntaxKind::L_BRACKET => "expected `[`",
        SyntaxKind::R_BRACKET => "expected `]`",
        SyntaxKind::LT => "expected `<`",
        SyntaxKind::GT => "expected `>`",
        SyntaxKind::FN_KW => "expected `fn`",
        SyntaxKind::MODULE_KW => "expected `module`",
        SyntaxKind::IN_KW => "expected `in`",
        SyntaxKind::THEN_KW => "expected `then`",
        SyntaxKind::ELSE_KW => "expected `else`",
        SyntaxKind::LAMBDA => "expected `\u{03bb}`",
        SyntaxKind::COMMA => "expected `,`",
        SyntaxKind::BANG => "expected `!`",
        SyntaxKind::USE_KW => "expected `use`",
        SyntaxKind::TYPE_KW => "expected `type`",
        SyntaxKind::ACTOR_KW => "expected `actor`",
        SyntaxKind::SUPERVISOR_KW => "expected `supervisor`",
        SyntaxKind::EFFECT_KW => "expected `effect`",
        SyntaxKind::TOOL_KW => "expected `tool`",
        SyntaxKind::EXTERN_KW => "expected `extern`",
        SyntaxKind::PIPE => "expected `|`",
        _ => "expected token",
    }
}

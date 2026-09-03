// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Recursive descent parser.
//!
//! Consumes a token stream from [`hird_lex::Lexer`], synthesises
//! whitespace tokens for gaps, and builds a cstree green tree.

use alloc::vec::Vec;

use cstree::build::{Checkpoint, GreenNodeBuilder};
use cstree::green::GreenNode;
use cstree::interning::new_interner;
use cstree::syntax::{ResolvedNode, SyntaxNode};
use hird_lex::{Lexer, Span, Token};

use crate::diagnostic::{DiagnosticCode, ParseDiagnostic};
use crate::syntax_kind::SyntaxKind;

/// Result of parsing a source file.
#[derive(Debug)]
pub struct ParseResult {
    /// Resolved CST root. Owns the token interner, so token text resolves
    /// without re-supplying the source.
    syntax: ResolvedNode<SyntaxKind>,
    /// Diagnostics emitted during parsing.
    diagnostics: Vec<ParseDiagnostic>,
}

impl ParseResult {
    /// Returns the resolved syntax tree root. Token text resolves directly
    /// (e.g. [`cstree::syntax::ResolvedToken::text`]); this is the entry point
    /// for typed AST projection.
    #[must_use]
    pub fn syntax(&self) -> &ResolvedNode<SyntaxKind> {
        &self.syntax
    }

    /// Returns the root green node.
    #[must_use]
    pub fn green(&self) -> &GreenNode {
        self.syntax.green()
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
    let (green, cache) = parser.builder.finish();
    let interner = cache
        .and_then(|cache| cache.into_interner())
        .unwrap_or_else(new_interner);
    let syntax = SyntaxNode::<SyntaxKind>::new_root_with_resolver(green, interner);
    ParseResult {
        syntax,
        diagnostics: parser.diagnostics,
    }
}

/// Maximum recursion depth for nested types, expressions, and patterns
/// before parsing aborts with [`DiagnosticCode::P0004`].
const MAX_NESTING: u32 = 256;

/// Associativity of an infix operator.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Assoc {
    /// `a ⊕ b ⊕ c` groups as `(a ⊕ b) ⊕ c`.
    Left,
    /// `a ⊕ b ⊕ c` is rejected; the operands must be parenthesised.
    None,
}

/// Recursive-descent parser over a lexed token stream, building a cstree
/// green tree.
struct Parser<'src, 'tok> {
    /// The source text, for slicing token and whitespace runs.
    source: &'src str,
    /// File identifier stamped into emitted [`Span`]s.
    source_id: u32,
    /// The full token stream, including trivia.
    tokens: &'tok [Token],
    /// Index of the next unconsumed token in `tokens`.
    pos: usize,
    /// End offset of the last emitted token, for synthesising whitespace.
    prev_end: u32,
    /// Current recursion depth, bounded by [`MAX_NESTING`].
    depth: u32,
    /// Accumulates the green tree.
    builder: GreenNodeBuilder<'static, 'static, SyntaxKind>,
    /// Errors collected during parsing.
    diagnostics: Vec<ParseDiagnostic>,
}

impl<'src, 'tok> Parser<'src, 'tok> {
    /// Creates a parser positioned at the first token.
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

    /// Whether `kind` is trivia (a comment) skipped between significant tokens.
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

    /// Whether the current token is `kind`.
    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    /// Whether the current token is an identifier spelled `text` (a contextual
    /// keyword such as `as`).
    fn at_contextual(&self, text: &str) -> bool {
        self.nth_is_contextual(0, text)
    }

    /// Whether the `n`th token past trivia is an identifier spelled `text`.
    fn nth_is_contextual(&self, n: usize, text: &str) -> bool {
        let mut pos = self.pos;
        let mut seen = 0;
        while pos < self.tokens.len() {
            let kind = SyntaxKind::from(self.tokens[pos].kind);
            if !Self::is_trivia(kind) {
                if seen == n {
                    return kind == SyntaxKind::IDENT
                        && self.tokens[pos].span.text(self.source) == text;
                }
                seen += 1;
            }
            pos += 1;
        }
        false
    }

    /// Span of the current token, or an empty span at end of input.
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

    /// Emits the current token verbatim with its leading whitespace and
    /// advances, without skipping trivia first.
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

    /// Emits the run of trivia tokens at the cursor.
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

    /// Emits leading trivia then the next significant token under `kind`
    /// instead of its lexed kind: a contextual keyword (`alias`) lexed as an
    /// identifier lands in the tree as the keyword it is in this position.
    fn bump_as(&mut self, kind: SyntaxKind) {
        self.eat_trivia();
        if self.pos >= self.tokens.len() {
            return;
        }
        let tok = self.tokens[self.pos];
        self.emit_whitespace_before(tok.span.start);
        self.builder.token(kind, tok.span.text(self.source));
        self.prev_end = tok.span.end;
        self.pos += 1;
    }

    /// Consumes the current token if it is `kind`; returns whether it matched.
    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consumes `kind` if present, otherwise emits an "expected" diagnostic.
    /// Returns whether it matched.
    fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            self.emit(DiagnosticCode::P0001, expected_msg(kind), None);
            false
        }
    }

    /// Push a diagnostic anchored at the current token's span.
    fn emit(&mut self, code: DiagnosticCode, message: &'static str, help: Option<&'static str>) {
        self.diagnostics.push(ParseDiagnostic {
            code,
            span: self.current_span(),
            message,
            help,
        });
    }

    /// Whether `kind` begins a top-level declaration.
    fn is_decl_keyword(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::FN_KW
                | SyntaxKind::TYPE_KW
                | SyntaxKind::ACTOR_KW
                | SyntaxKind::SUPERVISOR_KW
                | SyntaxKind::EFFECT_KW
                | SyntaxKind::TOOL_KW
                | SyntaxKind::EXTERN_KW
                | SyntaxKind::USE_KW
                | SyntaxKind::PUB_KW
        )
    }

    /// Whether the current token begins a top-level declaration.
    fn at_decl_keyword(&self) -> bool {
        Self::is_decl_keyword(self.current())
    }

    /// Whether the current token is a recovery synchronisation point: a
    /// declaration keyword, a closing delimiter, a list-separating comma, or
    /// end of input. Recovery skips *up to* one of these so an enclosing
    /// production can resume on the token.
    fn at_sync_point(&self) -> bool {
        let kind = self.current();
        Self::is_decl_keyword(kind)
            || matches!(
                kind,
                SyntaxKind::R_PAREN
                    | SyntaxKind::R_BRACE
                    | SyntaxKind::R_BRACKET
                    | SyntaxKind::COMMA
                    | SyntaxKind::EOF
            )
    }

    /// Report an error at the current token and, unless already at a
    /// synchronisation point (see `at_sync_point`), skip exactly that token into
    /// an `ERROR` node. Used where skipping a longer run would consume a
    /// delimiter a caller still needs — type and pattern positions; expression
    /// and declaration positions use `recover_to_sync` / `recover_decl`.
    fn error_bump(
        &mut self,
        code: DiagnosticCode,
        message: &'static str,
        help: Option<&'static str>,
    ) {
        self.emit(code, message, help);
        if self.at_sync_point() {
            return;
        }
        self.start_node(SyntaxKind::ERROR);
        self.bump();
        self.finish_node();
    }

    /// Recover from an unexpected token where an expression is expected: report
    /// the error, then skip a run of stray tokens into a single `ERROR` node up
    /// to the next synchronisation point (see `at_sync_point`). When already at
    /// a sync point, only the diagnostic is emitted — no tokens are consumed —
    /// so the enclosing delimited production (or the declaration loop) resumes
    /// on that token.
    fn recover_to_sync(
        &mut self,
        code: DiagnosticCode,
        message: &'static str,
        help: Option<&'static str>,
    ) {
        self.emit(code, message, help);
        if self.at_sync_point() {
            return;
        }
        self.start_node(SyntaxKind::ERROR);
        self.bump();
        while !self.at_sync_point() {
            self.bump();
        }
        self.finish_node();
    }

    /// Recover from an unexpected token where a top-level declaration is
    /// expected: report the error, then skip the malformed run into a single
    /// `ERROR` node up to the next declaration keyword (or end of input). A
    /// stray closing delimiter or comma is consumed as junk here — at the top
    /// level no enclosing production owns it — so the declaration loop always
    /// makes progress.
    fn recover_decl(&mut self, message: &'static str, help: Option<&'static str>) {
        self.emit(DiagnosticCode::P0002, message, help);
        self.start_node(SyntaxKind::ERROR);
        self.bump();
        while !self.at_decl_keyword() && self.current() != SyntaxKind::EOF {
            self.bump();
        }
        self.finish_node();
    }

    /// Whether the [`MAX_NESTING`] limit is reached, emitting
    /// [`DiagnosticCode::P0004`] when it is.
    fn too_deep(&mut self) -> bool {
        if self.depth >= MAX_NESTING {
            self.emit(DiagnosticCode::P0004, "nesting depth limit reached", None);
            return true;
        }
        false
    }

    // ── tree construction ───────────────────────────────────────

    /// Opens a new CST node of `kind`.
    fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(kind);
    }

    /// Closes the most recently opened node.
    fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    /// Marks the current position so a node can later wrap the children parsed
    /// after it (see [`Self::start_node_at`]).
    fn checkpoint(&mut self) -> Checkpoint {
        self.builder.checkpoint()
    }

    /// Retroactively opens a `kind` node at `checkpoint`, wrapping the children
    /// parsed since.
    fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        self.builder.start_node_at(checkpoint, kind);
    }

    // ── whitespace ──────────────────────────────────────────────

    /// Emits a [`SyntaxKind::WHITESPACE`] token for the gap before `next_start`,
    /// if any.
    fn emit_whitespace_before(&mut self, next_start: u32) {
        if next_start > self.prev_end {
            let ws = &self.source[self.prev_end as usize..next_start as usize];
            self.builder.token(SyntaxKind::WHITESPACE, ws);
            self.prev_end = next_start;
        }
    }

    /// Emits any whitespace between the last token and end of source.
    fn emit_trailing_whitespace(&mut self) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "source length checked at Lexer::new"
        )]
        let source_len = self.source.len() as u32;
        self.emit_whitespace_before(source_len);
    }

    /// Flushes any leftover tokens and trailing whitespace into the tree.
    fn drain_remaining(&mut self) {
        while self.pos < self.tokens.len() {
            self.bump_raw();
        }
        self.emit_trailing_whitespace();
    }

    // ── source file ─────────────────────────────────────────────

    /// Parses a whole file: an optional `module` header then top-level items,
    /// into a [`SyntaxKind::SOURCE_FILE`] node.
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

    /// Parses one top-level declaration, dispatching on the leading keyword (or
    /// `pub` and its successor); recovers on anything else.
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
            SyntaxKind::OPAQUE_KW => self.recover_decl(
                "`opaque` must follow `pub`",
                Some("an opaque type is declared `pub opaque type`"),
            ),
            SyntaxKind::PUB_KW => match self.nth(1) {
                SyntaxKind::FN_KW => self.parse_fn_decl(),
                SyntaxKind::TYPE_KW => self.parse_type_decl(),
                SyntaxKind::ACTOR_KW => self.parse_actor_decl(),
                SyntaxKind::SUPERVISOR_KW => self.parse_supervisor_decl(),
                SyntaxKind::EFFECT_KW => self.parse_effect_decl(),
                SyntaxKind::TOOL_KW => self.parse_tool_decl(),
                SyntaxKind::OPAQUE_KW if self.nth(2) == SyntaxKind::TYPE_KW => {
                    self.parse_type_decl();
                }
                SyntaxKind::OPAQUE_KW => self.recover_decl(
                    "`opaque` can only modify a `type`",
                    Some("an opaque type is declared `pub opaque type`"),
                ),
                _ => self.recover_decl(
                    "expected declaration after `pub`",
                    Some("`pub` must be followed by a declaration"),
                ),
            },
            _ => self.recover_decl(
                "expected declaration",
                Some(
                    "expected one of `fn`, `type`, `actor`, `supervisor`, `effect`, `tool`, \
                     `extern`, `use`, or `pub`",
                ),
            ),
        }
    }

    /// Parses an optional leading `pub` into a [`SyntaxKind::VISIBILITY`] node.
    fn parse_visibility(&mut self) {
        if self.at(SyntaxKind::PUB_KW) {
            self.start_node(SyntaxKind::VISIBILITY);
            self.bump();
            self.finish_node();
        }
    }

    // ── declarations ────────────────────────────────────────────

    /// `module Name`.
    fn parse_module_decl(&mut self) {
        self.start_node(SyntaxKind::MODULE_DECL);
        self.expect(SyntaxKind::MODULE_KW);
        self.expect(SyntaxKind::IDENT);
        self.finish_node();
    }

    /// A use import: a `.`-separated path, then either an `as` alias or a
    /// selective group — never both.
    ///
    /// ```text
    /// use Ets                  whole-module
    /// use Log as L             aliased
    /// use Ets.{Table, lookup}  selective (members brought in unqualified)
    /// ```
    fn parse_use_decl(&mut self) {
        self.start_node(SyntaxKind::USE_DECL);
        self.expect(SyntaxKind::USE_KW);
        self.parse_use_path();
        if self.at(SyntaxKind::DOT) {
            // A `.` after the path can only introduce a `.{ ... }` group.
            self.parse_use_group();
            // Selective and aliased forms are mutually exclusive. Absorb a
            // trailing `as Alias` so it does not trip the declaration loop, and
            // flag the combination.
            if self.at_contextual("as") {
                self.emit(
                    DiagnosticCode::P0002,
                    "a selective import cannot also be aliased",
                    Some("use either `M.{ a, b }` or `M as N`, not both"),
                );
                self.bump();
                self.expect(SyntaxKind::IDENT);
            }
        } else if self.at_contextual("as") {
            self.bump();
            self.expect(SyntaxKind::IDENT);
        }
        self.finish_node();
    }

    /// A `.`-separated path of identifiers (`A.B.C`). Stops before a `.{`
    /// selective group, leaving it for [`Self::parse_use_group`].
    fn parse_use_path(&mut self) {
        self.start_node(SyntaxKind::PATH);
        self.expect(SyntaxKind::IDENT);
        while self.at(SyntaxKind::DOT) && self.nth(1) == SyntaxKind::IDENT {
            self.bump(); // separator `.`
            self.bump(); // next segment
        }
        self.finish_node();
    }

    /// `.{ name, name, ... }` — a selective import group whose members are
    /// brought in unqualified. The group must name at least one member; the
    /// empty form (`.{}`) gets a tailored diagnostic.
    fn parse_use_group(&mut self) {
        self.start_node(SyntaxKind::USE_GROUP);
        self.expect(SyntaxKind::DOT);
        self.expect(SyntaxKind::L_BRACE);
        if self.at(SyntaxKind::R_BRACE) {
            self.emit(
                DiagnosticCode::P0002,
                "selective import group is empty",
                Some("list one or more names, e.g. `.{ Table, lookup }`"),
            );
        } else {
            self.expect(SyntaxKind::IDENT);
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_BRACE) {
                    break;
                }
                self.expect(SyntaxKind::IDENT);
            }
        }
        self.expect(SyntaxKind::R_BRACE);
        self.finish_node();
    }

    /// `fn name(params) → Ret ! {Effects} = body`.
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
        // The body follows a single `=`. Report a missing `=` with a tailored
        // hint instead of the generic "expected token", then parse the body
        // anyway so the rest of the declaration still projects.
        if !self.eat(SyntaxKind::EQ) {
            self.emit(
                DiagnosticCode::P0001,
                "missing `=` before function body",
                Some("insert `=` between the signature and the body"),
            );
        }
        self.parse_expr();
        self.finish_node();
    }

    /// A comma-separated parameter list.
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

    /// `name: Type`.
    fn parse_param(&mut self) {
        self.start_node(SyntaxKind::PARAM);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::COLON);
        self.parse_type_expr();
        self.finish_node();
    }

    /// `→ Type`.
    fn parse_return_type(&mut self) {
        self.start_node(SyntaxKind::RETURN_TYPE);
        self.expect(SyntaxKind::ARROW);
        self.parse_type_expr();
        self.finish_node();
    }

    /// `! { Effect, ... }` — an effect-row annotation.
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

    /// `[pub [opaque]] type Name<params> = Ctor | ...`, or
    /// `[pub] type alias Name<params> = Type`.
    ///
    /// `alias` is a contextual keyword: an identifier in the slot after `type`
    /// selects the alias form (a type name is `PascalCase`, so the two cannot
    /// collide) and is remapped to [`SyntaxKind::ALIAS_KW`]. The right-hand
    /// side of an alias is one type expression rather than a constructor
    /// list, and `opaque` on an alias is reported: there are no constructors
    /// to hide.
    ///
    /// `opaque` is consumed here but only reaches this point in the
    /// `pub opaque type` form: `parse_top_item` routes that form here and
    /// rejects `opaque` without a preceding `pub` or a following `type`.
    fn parse_type_decl(&mut self) {
        let checkpoint = self.checkpoint();
        self.parse_visibility();
        if self.at(SyntaxKind::OPAQUE_KW)
            && self.nth(1) == SyntaxKind::TYPE_KW
            && self.nth_is_contextual(2, "alias")
        {
            self.emit(
                DiagnosticCode::P0007,
                "a type alias cannot be `opaque`",
                Some(
                    "an alias names a shape and has no constructors to hide; write \
                     `pub type alias`, or declare an ADT with `pub opaque type`",
                ),
            );
        }
        self.eat(SyntaxKind::OPAQUE_KW);
        self.expect(SyntaxKind::TYPE_KW);
        if self.at_contextual("alias") {
            self.start_node_at(checkpoint, SyntaxKind::TYPE_ALIAS_DECL);
            self.bump_as(SyntaxKind::ALIAS_KW);
            self.expect(SyntaxKind::IDENT);
            if self.at(SyntaxKind::LT) {
                self.parse_type_params();
            }
            self.expect(SyntaxKind::EQ);
            self.parse_type_expr();
            self.finish_node();
            return;
        }
        self.start_node_at(checkpoint, SyntaxKind::TYPE_DECL);
        self.expect(SyntaxKind::IDENT);
        if self.at(SyntaxKind::LT) {
            self.parse_type_params();
        }
        self.expect(SyntaxKind::EQ);
        self.parse_constructors();
        self.finish_node();
    }

    /// A `|`-separated constructor list with an optional leading `|`. Shared by
    /// `type` declarations and actor `message` fields.
    fn parse_constructors(&mut self) {
        self.eat(SyntaxKind::PIPE);
        self.parse_constructor();
        while self.eat(SyntaxKind::PIPE) {
            self.parse_constructor();
        }
    }

    /// `<a, b, ...>` — a generic parameter list.
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

    /// `Name` or `Name(Type, ...)`.
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

    /// A comma-separated list of constructor field types.
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

    /// `actor Name { members } ! {Effects}`.
    fn parse_actor_decl(&mut self) {
        self.start_node(SyntaxKind::ACTOR_DECL);
        self.parse_visibility();
        self.expect(SyntaxKind::ACTOR_KW);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::L_BRACE);
        if !self.at(SyntaxKind::R_BRACE) {
            self.parse_actor_member();
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_BRACE) {
                    break;
                }
                self.parse_actor_member();
            }
        }
        self.expect(SyntaxKind::R_BRACE);
        if self.at(SyntaxKind::BANG) {
            self.parse_effect_ann();
        }
        self.finish_node();
    }

    /// An actor body member: a `handle` clause, or a `name:` field whose value
    /// is a function signature with a body (`init`), a type with an ADT tail
    /// (`message`), or a plain type (`state`). Shape — not field name — selects
    /// the form.
    fn parse_actor_member(&mut self) {
        if self.at(SyntaxKind::HANDLE_KW) {
            self.parse_actor_handler();
            return;
        }
        self.start_node(SyntaxKind::ACTOR_FIELD);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::COLON);
        if self.at(SyntaxKind::FN_KW) {
            self.parse_fn_sig();
            if !self.eat(SyntaxKind::EQ) {
                self.emit(
                    DiagnosticCode::P0001,
                    "missing `=` before init body",
                    Some("insert `=` between the signature and the body"),
                );
            }
            self.parse_expr();
        } else {
            self.parse_type_expr();
            if self.eat(SyntaxKind::EQ) {
                self.parse_constructors();
            }
        }
        self.finish_node();
    }

    /// `handle Pattern, State ! {Effects} = body`. The message pattern is
    /// followed by the current-state pattern; the body is a bare expression
    /// after `=` (no brace-delimited block form). A handler has no return
    /// type — the outcome is always `Next<State>` — so a `→ …` here is
    /// reported and skipped.
    fn parse_actor_handler(&mut self) {
        self.start_node(SyntaxKind::ACTOR_HANDLER);
        self.expect(SyntaxKind::HANDLE_KW);
        self.parse_pattern();
        if self.eat(SyntaxKind::COMMA) {
            self.parse_pattern();
        } else {
            self.emit(
                DiagnosticCode::P0001,
                "missing state pattern after the message pattern",
                Some("a handler binds the message, then the state: `handle Msg(x), st`"),
            );
        }
        if self.at(SyntaxKind::ARROW) {
            self.emit(
                DiagnosticCode::P0006,
                "a handler has no return type",
                Some("the outcome type is fixed by `state:` as `Next<State>`; remove `\u{2192} …`"),
            );
            self.parse_return_type();
        }
        if self.at(SyntaxKind::BANG) {
            self.parse_effect_ann();
        }
        if !self.eat(SyntaxKind::EQ) {
            self.emit(
                DiagnosticCode::P0001,
                "missing `=` before handler body",
                Some("insert `=` between the handler signature and the body"),
            );
        }
        self.parse_expr();
        self.finish_node();
    }

    /// `fn ( params ) ! {Effects}` — an unnamed, bodyless signature (an actor
    /// `init`). It has no return type — init always returns the state — so a
    /// `→ …` here is reported and skipped.
    fn parse_fn_sig(&mut self) {
        self.start_node(SyntaxKind::FN_SIG);
        self.expect(SyntaxKind::FN_KW);
        self.expect(SyntaxKind::L_PAREN);
        if !self.at(SyntaxKind::R_PAREN) {
            self.parse_param_list();
        }
        self.expect(SyntaxKind::R_PAREN);
        if self.at(SyntaxKind::ARROW) {
            self.emit(
                DiagnosticCode::P0006,
                "init has no return type",
                Some("init returns the `state:` type; remove `\u{2192} …`"),
            );
            self.parse_return_type();
        }
        if self.at(SyntaxKind::BANG) {
            self.parse_effect_ann();
        }
        self.finish_node();
    }

    /// `supervisor Name { fields }`.
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

    /// `name: expr`.
    fn parse_supervisor_field(&mut self) {
        self.start_node(SyntaxKind::SUPERVISOR_FIELD);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::COLON);
        self.parse_expr();
        self.finish_node();
    }

    /// `effect Name<params>`.
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

    /// `tool Name<params> : Input → Output ! {Effects}`. The type-parameter
    /// list and the trailing effect row are optional.
    fn parse_tool_decl(&mut self) {
        self.start_node(SyntaxKind::TOOL_DECL);
        self.parse_visibility();
        self.expect(SyntaxKind::TOOL_KW);
        self.expect(SyntaxKind::IDENT);
        if self.at(SyntaxKind::LT) {
            self.parse_type_params();
        }
        self.expect(SyntaxKind::COLON);
        self.parse_app_type();
        self.expect(SyntaxKind::ARROW);
        self.parse_type_expr();
        if self.at(SyntaxKind::BANG) {
            self.parse_effect_ann();
        }
        self.finish_node();
    }

    /// `extern fn name(params) → Ret`.
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

    /// Parses a type expression (depth-guarded entry point).
    fn parse_type_expr(&mut self) {
        if self.too_deep() {
            return;
        }
        self.depth += 1;
        self.parse_fn_type();
        self.depth -= 1;
    }

    /// Function type `A → B`, the lowest-precedence type form; a bare operand
    /// when no `→` follows.
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

    /// Type application `Ctor<Args>`, or a bare atom when no `<` follows.
    fn parse_app_type(&mut self) {
        let cp = self.checkpoint();
        self.parse_atom_type();
        if self.at(SyntaxKind::LT) {
            self.start_node_at(cp, SyntaxKind::APP_TYPE);
            self.parse_type_args();
            self.finish_node();
        }
    }

    /// An atomic type: a name, a record type, or a parenthesised type, tuple,
    /// or `()`.
    fn parse_atom_type(&mut self) {
        match self.current() {
            SyntaxKind::IDENT => {
                self.bump();
            }
            SyntaxKind::L_BRACE => self.parse_record_type(),
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
                self.error_bump(
                    DiagnosticCode::P0003,
                    "expected type",
                    Some("expected a type, e.g. `Int`, `List<a>`, or `(A, B)`"),
                );
            }
        }
    }

    /// `{ name: Type, ... }` — a structural record type. A `{` where a type is
    /// expected always begins one; braces never delimit anything else in type
    /// position.
    fn parse_record_type(&mut self) {
        self.start_node(SyntaxKind::RECORD_TYPE);
        self.expect(SyntaxKind::L_BRACE);
        if !self.at(SyntaxKind::R_BRACE) {
            self.parse_record_type_field();
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_BRACE) {
                    break;
                }
                self.parse_record_type_field();
            }
        }
        self.expect(SyntaxKind::R_BRACE);
        self.finish_node();
    }

    /// `name: Type`.
    fn parse_record_type_field(&mut self) {
        self.start_node(SyntaxKind::RECORD_TYPE_FIELD);
        self.expect_field_name();
        self.expect(SyntaxKind::COLON);
        self.parse_type_expr();
        self.finish_node();
    }

    /// `<A, B, ...>` — type-application arguments.
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

    /// Parses an expression: a Pratt expression, optionally followed by `;`
    /// and another expression. Sequencing is right-associative and binds
    /// looser than every operator, so `a; b; c` is `a; (b; c)` and a `;`
    /// inside a `let` body, an `if` branch, or a match arm belongs to that
    /// body.
    fn parse_expr(&mut self) {
        if self.too_deep() {
            return;
        }
        self.depth += 1;
        let cp = self.checkpoint();
        self.parse_expr_bp(0);
        if self.at(SyntaxKind::SEMICOLON) {
            self.start_node_at(cp, SyntaxKind::SEQ_EXPR);
            self.bump();
            self.parse_expr();
            self.finish_node();
        }
        self.depth -= 1;
    }

    /// Pratt loop: parses an expression whose operators bind at least as
    /// tightly as `min_bp`, handling field access, application, and infix
    /// operators.
    fn parse_expr_bp(&mut self, min_bp: u8) {
        if self.too_deep() {
            return;
        }
        self.depth += 1;
        let lhs = self.checkpoint();
        self.parse_prefix_expr();

        // The binding power of the last non-associative operator consumed at
        // this tier, if any. A second operator of the same tier is a forbidden
        // chain (`a == b == c`); the operands must be parenthesised.
        let mut prev_nonassoc_bp: Option<u8> = None;

        loop {
            // Bound operator recursion: the application branch consumes no
            // token before recursing, so without this it spins at the limit.
            if self.depth >= MAX_NESTING {
                break;
            }

            if Self::field_bp() >= min_bp && self.at(SyntaxKind::DOT) {
                self.start_node_at(lhs, SyntaxKind::FIELD_EXPR);
                self.bump();
                self.expect(SyntaxKind::IDENT);
                self.finish_node();
                continue;
            }

            if Self::app_bp() >= min_bp && self.at_app_arg_start() {
                self.start_node_at(lhs, SyntaxKind::APP_EXPR);
                self.parse_expr_bp(Self::app_bp() + 1);
                self.finish_node();
                continue;
            }

            let Some((left_bp, right_bp, assoc)) = Self::infix_bp(self.current()) else {
                break;
            };
            if left_bp < min_bp {
                break;
            }

            if assoc == Assoc::None && prev_nonassoc_bp == Some(left_bp) {
                self.emit(
                    DiagnosticCode::P0005,
                    "non-associative operator cannot be chained; parenthesise",
                    None,
                );
            }
            prev_nonassoc_bp = match assoc {
                Assoc::None => Some(left_bp),
                Assoc::Left => None,
            };

            self.start_node_at(lhs, SyntaxKind::BIN_EXPR);
            self.bump();
            self.parse_expr_bp(right_bp);
            self.finish_node();
        }
        self.depth -= 1;
    }

    /// A prefix-position expression: `let`, `λ`, `if`, `match`, `handle`, or
    /// one of the keyword forms (`spawn`, `supervise`, `stand`, `clock`,
    /// `self`, `child`, `send`, `request`, `schedule`, `reply`,
    /// `crash!`/`panic!`), otherwise an atom.
    fn parse_prefix_expr(&mut self) {
        match self.current() {
            SyntaxKind::LET_KW => self.parse_let_expr(),
            SyntaxKind::LAMBDA => self.parse_lambda_expr(),
            SyntaxKind::IF_KW => self.parse_if_expr(),
            SyntaxKind::MATCH_KW => self.parse_match_expr(),
            SyntaxKind::HANDLE_KW => self.parse_handle_expr(),
            SyntaxKind::INSTALL_KW => self.parse_install_expr(),
            SyntaxKind::SPAWN_KW => self.parse_spawn_expr(),
            SyntaxKind::SUPERVISE_KW => self.parse_supervise_expr(),
            SyntaxKind::STAND_KW => self.parse_nullary_form(SyntaxKind::STAND_EXPR),
            // `clock` is contextual (like `as`): the form only as `clock()`,
            // an ordinary identifier elsewhere, so `clock: Clock` stays a
            // natural parameter name.
            SyntaxKind::IDENT
                if self.at_contextual("clock")
                    && self.nth(1) == SyntaxKind::L_PAREN
                    && self.nth(2) == SyntaxKind::R_PAREN =>
            {
                self.parse_nullary_form(SyntaxKind::CLOCK_EXPR);
            }
            SyntaxKind::SELF_KW => self.parse_nullary_form(SyntaxKind::SELF_EXPR),
            SyntaxKind::CHILD_KW => self.parse_child_expr(),
            SyntaxKind::SEND_KW => self.parse_call_form(SyntaxKind::SEND_EXPR, 2, 0),
            SyntaxKind::REQUEST_KW => self.parse_call_form(SyntaxKind::REQUEST_EXPR, 2, 1),
            SyntaxKind::SCHEDULE_KW => self.parse_call_form(SyntaxKind::SCHEDULE_EXPR, 4, 0),
            SyntaxKind::REPLY_KW => self.parse_call_form(SyntaxKind::REPLY_EXPR, 2, 0),
            SyntaxKind::CRASH_KW | SyntaxKind::PANIC_KW => self.parse_crash_expr(),
            _ => self.parse_atom_expr(),
        }
    }

    /// Binding power of field access (`.`), the tightest-binding postfix form.
    const fn field_bp() -> u8 {
        50
    }

    /// Binding power of function application (juxtaposition).
    const fn app_bp() -> u8 {
        40
    }

    /// Binding power and associativity of an infix operator, or `None` if
    /// `kind` is not one. Left-associative operators bind tighter on the right
    /// (`right_bp = left_bp + 1`); non-associative operators share that shape
    /// but reject chaining at the same tier (see [`Self::parse_expr_bp`]).
    const fn infix_bp(kind: SyntaxKind) -> Option<(u8, u8, Assoc)> {
        match kind {
            SyntaxKind::STAR | SyntaxKind::SLASH => Some((30, 31, Assoc::Left)),
            SyntaxKind::PLUS | SyntaxKind::MINUS => Some((20, 21, Assoc::Left)),
            SyntaxKind::LT
            | SyntaxKind::LE
            | SyntaxKind::GT
            | SyntaxKind::GE
            | SyntaxKind::EQ_EQ
            | SyntaxKind::BANG_EQ => Some((10, 11, Assoc::None)),
            SyntaxKind::ANDAND => Some((8, 9, Assoc::Left)),
            SyntaxKind::OROR => Some((6, 7, Assoc::Left)),
            _ => None,
        }
    }

    /// Whether the current token can begin an application argument.
    fn at_app_arg_start(&self) -> bool {
        // `{` is excluded: a record literal is not a juxtaposition argument,
        // which keeps `{` free to disambiguate against a future block form.
        matches!(
            self.current(),
            SyntaxKind::IDENT
                | SyntaxKind::INT
                | SyntaxKind::FLOAT
                | SyntaxKind::STRING
                | SyntaxKind::L_PAREN
                | SyntaxKind::L_BRACKET
        )
    }

    /// `let name (: Type)? = value in body`.
    fn parse_let_expr(&mut self) {
        self.start_node(SyntaxKind::LET_EXPR);
        self.expect(SyntaxKind::LET_KW);
        self.parse_pattern();
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

    /// `λ x y ... → body`.
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

    /// `if cond then a else b`.
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

    /// `match scrutinee { pat → expr, ... }`.
    fn parse_match_expr(&mut self) {
        self.start_node(SyntaxKind::MATCH_EXPR);
        self.expect(SyntaxKind::MATCH_KW);
        self.parse_expr();
        self.expect(SyntaxKind::L_BRACE);
        if !self.at(SyntaxKind::R_BRACE) {
            self.parse_match_arm();
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_BRACE) {
                    break;
                }
                self.parse_match_arm();
            }
        }
        self.expect(SyntaxKind::R_BRACE);
        self.finish_node();
    }

    /// `pattern → expr`.
    fn parse_match_arm(&mut self) {
        self.start_node(SyntaxKind::MATCH_ARM);
        self.parse_pattern();
        self.expect(SyntaxKind::ARROW);
        self.parse_expr();
        self.finish_node();
    }

    /// `spawn(Actor, args…)` — a keyword form. The first argument is an actor
    /// name resolved in the actor namespace, not an expression; the remaining
    /// arguments are the actor's init arguments.
    fn parse_spawn_expr(&mut self) {
        self.start_node(SyntaxKind::SPAWN_EXPR);
        self.expect(SyntaxKind::SPAWN_KW);
        self.expect(SyntaxKind::L_PAREN);
        self.expect(SyntaxKind::IDENT);
        while self.eat(SyntaxKind::COMMA) {
            if self.at(SyntaxKind::R_PAREN) {
                break;
            }
            self.parse_expr();
        }
        self.expect(SyntaxKind::R_PAREN);
        self.finish_node();
    }

    /// `supervise(SupName)` — a keyword form. The argument is a supervisor
    /// name resolved in the supervisor namespace, not an expression.
    fn parse_supervise_expr(&mut self) {
        self.start_node(SyntaxKind::SUPERVISE_EXPR);
        self.expect(SyntaxKind::SUPERVISE_KW);
        self.expect(SyntaxKind::L_PAREN);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::R_PAREN);
        self.finish_node();
    }

    /// `stand()`, `clock()`, or `self()` — a keyword form taking no
    /// arguments, wrapped in `node`. The head token is consumed as is
    /// (`clock` is an `IDENT`).
    fn parse_nullary_form(&mut self, node: SyntaxKind) {
        self.start_node(node);
        self.bump();
        self.expect(SyntaxKind::L_PAREN);
        self.expect(SyntaxKind::R_PAREN);
        self.finish_node();
    }

    /// `child(SupName, child_id)` — a keyword form. The first argument is a
    /// supervisor name resolved in the supervisor namespace, the second one
    /// of its declared child ids; neither is an expression.
    fn parse_child_expr(&mut self) {
        self.start_node(SyntaxKind::CHILD_EXPR);
        self.expect(SyntaxKind::CHILD_KW);
        self.expect(SyntaxKind::L_PAREN);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::COMMA);
        self.expect(SyntaxKind::IDENT);
        self.expect(SyntaxKind::R_PAREN);
        self.finish_node();
    }

    /// A keyword form over expression arguments — `send(pid, msg)`,
    /// `request(pid, ctor[, timeout_ms])`, `schedule(clock, pid, msg,
    /// delay_ms)`, `reply(reply_to, value)` — with `required` arguments and up
    /// to `optional` more, wrapped in `node`.
    fn parse_call_form(&mut self, node: SyntaxKind, required: usize, optional: usize) {
        self.start_node(node);
        self.bump();
        self.expect(SyntaxKind::L_PAREN);
        for i in 0..required {
            if i > 0 {
                self.expect(SyntaxKind::COMMA);
            }
            self.parse_expr();
        }
        for _ in 0..optional {
            if !self.eat(SyntaxKind::COMMA) {
                break;
            }
            self.parse_expr();
        }
        self.expect(SyntaxKind::R_PAREN);
        self.finish_node();
    }

    /// `crash!(message)` or `panic!(message)` — the divergent primitive. The
    /// `!` is a required syntactic marker; the single argument is the crash
    /// message. Both spellings produce a `CRASH_EXPR`; `panic!` is an alias.
    fn parse_crash_expr(&mut self) {
        self.start_node(SyntaxKind::CRASH_EXPR);
        self.bump();
        self.expect(SyntaxKind::BANG);
        self.expect(SyntaxKind::L_PAREN);
        self.parse_expr();
        self.expect(SyntaxKind::R_PAREN);
        self.finish_node();
    }

    /// `handle { Effect → handler, ... } in body`.
    fn parse_handle_expr(&mut self) {
        self.start_node(SyntaxKind::HANDLE_EXPR);
        self.expect(SyntaxKind::HANDLE_KW);
        self.parse_handler_arms_in_body();
        self.finish_node();
    }

    /// `install { Effect → handler, ... } in body` — `handle`'s arm grammar
    /// behind the `install` keyword.
    fn parse_install_expr(&mut self) {
        self.start_node(SyntaxKind::INSTALL_EXPR);
        self.expect(SyntaxKind::INSTALL_KW);
        self.parse_handler_arms_in_body();
        self.finish_node();
    }

    /// `{ Effect → handler, ... } in body` — the shared tail of `handle` and
    /// `install` expressions.
    fn parse_handler_arms_in_body(&mut self) {
        self.expect(SyntaxKind::L_BRACE);
        if !self.at(SyntaxKind::R_BRACE) {
            self.parse_handle_arm();
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_BRACE) {
                    break;
                }
                self.parse_handle_arm();
            }
        }
        self.expect(SyntaxKind::R_BRACE);
        self.expect(SyntaxKind::IN_KW);
        self.parse_expr();
    }

    /// `Effect → handler`.
    fn parse_handle_arm(&mut self) {
        self.start_node(SyntaxKind::HANDLE_ARM);
        self.parse_app_type();
        self.expect(SyntaxKind::ARROW);
        self.parse_expr();
        self.finish_node();
    }

    /// An atomic expression: a literal, name, parenthesised or tuple
    /// expression, list, or record literal.
    fn parse_atom_expr(&mut self) {
        match self.current() {
            SyntaxKind::IDENT | SyntaxKind::INT | SyntaxKind::FLOAT | SyntaxKind::STRING => {
                self.bump();
            }
            SyntaxKind::L_PAREN => self.parse_paren_or_tuple_expr(),
            SyntaxKind::L_BRACKET => self.parse_list_lit(),
            SyntaxKind::L_BRACE => self.parse_record_lit(),
            _ => {
                self.recover_to_sync(DiagnosticCode::P0002, "expected expression", None);
            }
        }
    }

    /// `(e)` is parenthesised, `(a, b, ...)` is a tuple, and `()` is unit.
    fn parse_paren_or_tuple_expr(&mut self) {
        let cp = self.checkpoint();
        self.bump();
        if self.at(SyntaxKind::R_PAREN) {
            self.start_node_at(cp, SyntaxKind::TUPLE_LIT);
            self.bump();
            self.finish_node();
            return;
        }
        self.parse_expr();
        if self.at(SyntaxKind::COMMA) {
            self.start_node_at(cp, SyntaxKind::TUPLE_LIT);
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_PAREN) {
                    break;
                }
                self.parse_expr();
            }
            self.expect(SyntaxKind::R_PAREN);
            self.finish_node();
        } else {
            self.start_node_at(cp, SyntaxKind::PAREN_EXPR);
            self.expect(SyntaxKind::R_PAREN);
            self.finish_node();
        }
    }

    /// `[a, b, ...]` — a list literal.
    fn parse_list_lit(&mut self) {
        self.start_node(SyntaxKind::LIST_LIT);
        self.expect(SyntaxKind::L_BRACKET);
        if !self.at(SyntaxKind::R_BRACKET) {
            self.parse_expr();
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_BRACKET) {
                    break;
                }
                self.parse_expr();
            }
        }
        self.expect(SyntaxKind::R_BRACKET);
        self.finish_node();
    }

    /// `{` always begins a record literal here; there is no block-expression
    /// form to disambiguate against. Fields use `name: expr`.
    fn parse_record_lit(&mut self) {
        self.start_node(SyntaxKind::RECORD_LIT);
        self.expect(SyntaxKind::L_BRACE);
        if !self.at(SyntaxKind::R_BRACE) {
            self.parse_record_field();
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_BRACE) {
                    break;
                }
                self.parse_record_field();
            }
        }
        self.expect(SyntaxKind::R_BRACE);
        self.finish_node();
    }

    /// `name: expr`.
    fn parse_record_field(&mut self) {
        self.start_node(SyntaxKind::RECORD_FIELD);
        self.expect_field_name();
        self.expect(SyntaxKind::COLON);
        self.parse_expr();
        self.finish_node();
    }

    /// Consume a record field name. A keyword spelling (e.g. `actor`) is a
    /// valid field name here; the `name :` shape is unambiguous.
    fn expect_field_name(&mut self) {
        if self.at(SyntaxKind::IDENT) || is_keyword(self.current()) {
            self.bump();
        } else {
            self.expect(SyntaxKind::IDENT);
        }
    }

    // ── patterns ────────────────────────────────────────────────

    /// Parses a pattern (depth-guarded entry point).
    fn parse_pattern(&mut self) {
        if self.too_deep() {
            return;
        }
        self.depth += 1;
        self.parse_pattern_inner();
        self.depth -= 1;
    }

    /// A pattern: a literal, tuple, wildcard `_`, constructor, or binding.
    fn parse_pattern_inner(&mut self) {
        match self.current() {
            SyntaxKind::INT | SyntaxKind::FLOAT | SyntaxKind::STRING => {
                self.start_node(SyntaxKind::LITERAL_PAT);
                self.bump();
                self.finish_node();
            }
            SyntaxKind::L_PAREN => self.parse_tuple_pattern(),
            SyntaxKind::IDENT => {
                if self.current_ident_text() == "_" {
                    self.start_node(SyntaxKind::WILDCARD_PAT);
                    self.bump();
                    self.finish_node();
                } else if self.current_ident_is_ctor() {
                    self.parse_constructor_pattern();
                } else {
                    self.start_node(SyntaxKind::BIND_PAT);
                    self.bump();
                    self.finish_node();
                }
            }
            _ => {
                self.error_bump(DiagnosticCode::P0002, "expected pattern", None);
            }
        }
    }

    /// `(p)` is a grouped pattern (no wrapper node), `(a, b, ...)` is a tuple
    /// pattern, and `()` is the empty tuple pattern.
    fn parse_tuple_pattern(&mut self) {
        let cp = self.checkpoint();
        self.bump();
        if self.at(SyntaxKind::R_PAREN) {
            self.start_node_at(cp, SyntaxKind::TUPLE_PAT);
            self.bump();
            self.finish_node();
            return;
        }
        self.parse_pattern();
        if self.at(SyntaxKind::COMMA) {
            self.start_node_at(cp, SyntaxKind::TUPLE_PAT);
            while self.eat(SyntaxKind::COMMA) {
                if self.at(SyntaxKind::R_PAREN) {
                    break;
                }
                self.parse_pattern();
            }
            self.expect(SyntaxKind::R_PAREN);
            self.finish_node();
        } else {
            self.expect(SyntaxKind::R_PAREN);
        }
    }

    /// `Ctor` or `Ctor(p, ...)`.
    fn parse_constructor_pattern(&mut self) {
        self.start_node(SyntaxKind::CONSTRUCTOR_PAT);
        self.bump();
        if self.eat(SyntaxKind::L_PAREN) {
            if !self.at(SyntaxKind::R_PAREN) {
                self.parse_pattern();
                while self.eat(SyntaxKind::COMMA) {
                    if self.at(SyntaxKind::R_PAREN) {
                        break;
                    }
                    self.parse_pattern();
                }
            }
            self.expect(SyntaxKind::R_PAREN);
        }
        self.finish_node();
    }

    /// Text of the current token (assumed an identifier).
    fn current_ident_text(&self) -> &str {
        self.current_span().text(self.source)
    }

    /// Whether the current identifier is `PascalCase` (constructor-shaped).
    ///
    /// Classification is by the first non-underscore byte, mirroring the
    /// lexer's naming rule; all-underscore identifiers are not constructors.
    fn current_ident_is_ctor(&self) -> bool {
        self.current_ident_text()
            .bytes()
            .find(|b| *b != b'_')
            .is_some_and(|b| b.is_ascii_uppercase())
    }
}

/// Whether `kind` is a reserved keyword token. Used to allow keyword spellings
/// as record field names, where the `name :` shape leaves no ambiguity.
fn is_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LET_KW
            | SyntaxKind::FN_KW
            | SyntaxKind::MATCH_KW
            | SyntaxKind::TYPE_KW
            | SyntaxKind::ACTOR_KW
            | SyntaxKind::SUPERVISOR_KW
            | SyntaxKind::EFFECT_KW
            | SyntaxKind::TOOL_KW
            | SyntaxKind::HANDLE_KW
            | SyntaxKind::INSTALL_KW
            | SyntaxKind::SPAWN_KW
            | SyntaxKind::SEND_KW
            | SyntaxKind::REQUEST_KW
            | SyntaxKind::REPLY_KW
            | SyntaxKind::CRASH_KW
            | SyntaxKind::PANIC_KW
            | SyntaxKind::USE_KW
            | SyntaxKind::MODULE_KW
            | SyntaxKind::PUB_KW
            | SyntaxKind::OPAQUE_KW
            | SyntaxKind::EXTERN_KW
            | SyntaxKind::IF_KW
            | SyntaxKind::THEN_KW
            | SyntaxKind::ELSE_KW
            | SyntaxKind::IN_KW
    )
}

/// Static "expected ..." message for `kind`, used by [`Parser::expect`].
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

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hirð typed AST data structures.
//!
//! A typed projection over the resolved [`cstree`] tree produced by
//! [`hird_parse`]. Each node type is a thin newtype around a
//! [`ResolvedNode`]; casting checks the [`SyntaxKind`] and accessors walk the
//! children. Token text resolves directly (the tree owns its interner), so
//! accessors return `&str` without re-supplying the source.
//!
//! The projection covers declarations and expressions. Type expressions and
//! patterns are not yet projected; where an accessor would return one, the
//! underlying node is reachable through [`AstNode::syntax`].
//!
//! # Entry point
//!
//! ```
//! use hird_ast::{AstNode, SourceFile};
//!
//! let parsed = hird_parse::parse("fn answer() = 42", 0);
//! let file = SourceFile::cast(parsed.syntax().clone()).unwrap();
//! assert_eq!(file.declarations().count(), 1);
//! ```

#![no_std]

use cstree::syntax::{ResolvedElementRef, ResolvedNode, ResolvedToken};
use cstree::util::NodeOrToken;
use hird_parse::SyntaxKind;

/// A text-resolving CST node specialised to Hirð's [`SyntaxKind`].
pub type SyntaxNode = ResolvedNode<SyntaxKind>;

/// A text-resolving CST token specialised to Hirð's [`SyntaxKind`].
pub type SyntaxToken = ResolvedToken<SyntaxKind>;

/// A typed view over a single [`SyntaxKind`] of CST node.
///
/// Implementors are thin newtypes around a [`SyntaxNode`]; [`cast`](Self::cast)
/// succeeds only when the node's kind matches.
pub trait AstNode: Sized {
    /// Whether a node of `kind` can be cast to this type.
    fn can_cast(kind: SyntaxKind) -> bool;

    /// Wraps `syntax` if its kind matches; otherwise returns `None`.
    fn cast(syntax: SyntaxNode) -> Option<Self>;

    /// The underlying CST node.
    fn syntax(&self) -> &SyntaxNode;
}

// ── support ─────────────────────────────────────────────────────

/// The first child node that casts to `N`.
fn child<N: AstNode>(node: &SyntaxNode) -> Option<N> {
    node.children().find_map(|c| N::cast(c.clone()))
}

/// Every child node that casts to `N`, in source order.
fn children<N: AstNode>(node: &SyntaxNode) -> impl Iterator<Item = N> + '_ {
    node.children().filter_map(|c| N::cast(c.clone()))
}

/// The first direct child token of the given `kind`.
fn token(node: &SyntaxNode, kind: SyntaxKind) -> Option<&SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == kind)
}

/// The text of the first direct child token of the given `kind`.
fn token_text(node: &SyntaxNode, kind: SyntaxKind) -> Option<&str> {
    token(node, kind).map(|t| t.text())
}

/// The conventional name of a declaration: its first `IDENT` token.
fn name(node: &SyntaxNode) -> Option<&str> {
    token_text(node, SyntaxKind::IDENT)
}

/// Whether `node` carries a `pub` visibility modifier.
fn is_pub(node: &SyntaxNode) -> bool {
    node.children().any(|c| c.kind() == SyntaxKind::VISIBILITY)
}

fn element_kind(element: ResolvedElementRef<'_, SyntaxKind>) -> SyntaxKind {
    match element {
        NodeOrToken::Node(n) => n.kind(),
        NodeOrToken::Token(t) => t.kind(),
    }
}

/// The first expression operand among a node's children/tokens.
fn first_expr(node: &SyntaxNode) -> Option<Expr> {
    node.children_with_tokens().find_map(Expr::cast_element)
}

/// Every expression operand among a node's children/tokens, in source order.
fn exprs(node: &SyntaxNode) -> impl Iterator<Item = Expr> + '_ {
    node.children_with_tokens().filter_map(Expr::cast_element)
}

/// The first expression operand appearing after the token `kw`.
fn expr_after(node: &SyntaxNode, kw: SyntaxKind) -> Option<Expr> {
    node.children_with_tokens()
        .skip_while(|e| element_kind(*e) != kw)
        .skip(1)
        .find_map(Expr::cast_element)
}

/// Defines a newtype over one node kind and its [`AstNode`] impl.
macro_rules! ast_node {
    ($(#[$doc:meta])* $name:ident => $kind:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone)]
        pub struct $name(SyntaxNode);

        impl AstNode for $name {
            fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }

            fn cast(syntax: SyntaxNode) -> Option<Self> {
                if Self::can_cast(syntax.kind()) {
                    Some(Self(syntax))
                } else {
                    None
                }
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

// ── root ────────────────────────────────────────────────────────

ast_node! {
    /// The root of a parsed source file.
    SourceFile => SOURCE_FILE
}

impl SourceFile {
    /// The leading `module` declaration, if present.
    #[must_use]
    pub fn module(&self) -> Option<ModuleDecl> {
        child(&self.0)
    }

    /// All top-level declarations, in source order.
    pub fn declarations(&self) -> impl Iterator<Item = Decl> + '_ {
        children(&self.0)
    }
}

// ── declarations ────────────────────────────────────────────────

ast_node! {
    /// A module declaration (`module Name`).
    ModuleDecl => MODULE_DECL
}

impl ModuleDecl {
    /// The module name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }
}

ast_node! {
    /// A use import (`use Path::To::Item as Alias`).
    UseDecl => USE_DECL
}

impl UseDecl {
    /// The imported path.
    #[must_use]
    pub fn path(&self) -> Option<Path> {
        child(&self.0)
    }

    /// The local alias introduced by `as`, if any.
    #[must_use]
    pub fn alias(&self) -> Option<&str> {
        // `as` is a contextual keyword lexed as an `IDENT`; the alias is the
        // next non-trivia token after it. (Path segments are nested in `Path`,
        // not direct children here.)
        self.0
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| !is_trivia(t.kind()))
            .skip_while(|t| t.text() != "as")
            .nth(1)
            .map(|t| t.text())
    }
}

ast_node! {
    /// A function declaration.
    FnDecl => FN_DECL
}

impl FnDecl {
    /// Whether the function is exported (`pub`).
    #[must_use]
    pub fn is_pub(&self) -> bool {
        is_pub(&self.0)
    }

    /// The function name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }

    /// The declared parameters, in order.
    pub fn params(&self) -> impl Iterator<Item = Param> + '_ {
        params(&self.0)
    }

    /// The body expression (after `=`).
    #[must_use]
    pub fn body(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::EQ)
    }
}

ast_node! {
    /// An algebraic data type declaration.
    TypeDecl => TYPE_DECL
}

impl TypeDecl {
    /// Whether the type is exported (`pub`).
    #[must_use]
    pub fn is_pub(&self) -> bool {
        is_pub(&self.0)
    }

    /// The type name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }

    /// The constructors of the data type, in order.
    pub fn constructors(&self) -> impl Iterator<Item = Constructor> + '_ {
        children(&self.0)
    }
}

ast_node! {
    /// An effect declaration (`effect Name<params>`).
    EffectDecl => EFFECT_DECL
}

impl EffectDecl {
    /// Whether the effect is exported (`pub`).
    #[must_use]
    pub fn is_pub(&self) -> bool {
        is_pub(&self.0)
    }

    /// The effect name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }
}

ast_node! {
    /// A tool declaration (`tool Name : Input -> Output`).
    ToolDecl => TOOL_DECL
}

impl ToolDecl {
    /// Whether the tool is exported (`pub`).
    #[must_use]
    pub fn is_pub(&self) -> bool {
        is_pub(&self.0)
    }

    /// The tool name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }
}

ast_node! {
    /// An extern function declaration (`extern fn name(params) -> Type`).
    ExternDecl => EXTERN_DECL
}

impl ExternDecl {
    /// The function name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }

    /// The declared parameters, in order.
    pub fn params(&self) -> impl Iterator<Item = Param> + '_ {
        params(&self.0)
    }
}

ast_node! {
    /// An actor declaration. Its body is parsed but not yet typed; reach it via
    /// [`AstNode::syntax`].
    ActorDecl => ACTOR_DECL
}

impl ActorDecl {
    /// Whether the actor is exported (`pub`).
    #[must_use]
    pub fn is_pub(&self) -> bool {
        is_pub(&self.0)
    }

    /// The actor name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }
}

ast_node! {
    /// A supervisor declaration. Its body is parsed but not yet typed; reach it
    /// via [`AstNode::syntax`].
    SupervisorDecl => SUPERVISOR_DECL
}

impl SupervisorDecl {
    /// Whether the supervisor is exported (`pub`).
    #[must_use]
    pub fn is_pub(&self) -> bool {
        is_pub(&self.0)
    }

    /// The supervisor name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }
}

/// Any top-level declaration.
#[derive(Debug, Clone)]
pub enum Decl {
    /// `module Name`
    Module(ModuleDecl),
    /// `use Path`
    Use(UseDecl),
    /// `fn name(..) = ..`
    Fn(FnDecl),
    /// `type Name = ..`
    Type(TypeDecl),
    /// `effect Name`
    Effect(EffectDecl),
    /// `tool Name : ..`
    Tool(ToolDecl),
    /// `extern fn name(..)`
    Extern(ExternDecl),
    /// `actor Name { .. }`
    Actor(ActorDecl),
    /// `supervisor Name { .. }`
    Supervisor(SupervisorDecl),
}

impl AstNode for Decl {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::MODULE_DECL
                | SyntaxKind::USE_DECL
                | SyntaxKind::FN_DECL
                | SyntaxKind::TYPE_DECL
                | SyntaxKind::EFFECT_DECL
                | SyntaxKind::TOOL_DECL
                | SyntaxKind::EXTERN_DECL
                | SyntaxKind::ACTOR_DECL
                | SyntaxKind::SUPERVISOR_DECL
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        let decl = match syntax.kind() {
            SyntaxKind::MODULE_DECL => Self::Module(ModuleDecl(syntax)),
            SyntaxKind::USE_DECL => Self::Use(UseDecl(syntax)),
            SyntaxKind::FN_DECL => Self::Fn(FnDecl(syntax)),
            SyntaxKind::TYPE_DECL => Self::Type(TypeDecl(syntax)),
            SyntaxKind::EFFECT_DECL => Self::Effect(EffectDecl(syntax)),
            SyntaxKind::TOOL_DECL => Self::Tool(ToolDecl(syntax)),
            SyntaxKind::EXTERN_DECL => Self::Extern(ExternDecl(syntax)),
            SyntaxKind::ACTOR_DECL => Self::Actor(ActorDecl(syntax)),
            SyntaxKind::SUPERVISOR_DECL => Self::Supervisor(SupervisorDecl(syntax)),
            _ => return None,
        };
        Some(decl)
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Module(n) => n.syntax(),
            Self::Use(n) => n.syntax(),
            Self::Fn(n) => n.syntax(),
            Self::Type(n) => n.syntax(),
            Self::Effect(n) => n.syntax(),
            Self::Tool(n) => n.syntax(),
            Self::Extern(n) => n.syntax(),
            Self::Actor(n) => n.syntax(),
            Self::Supervisor(n) => n.syntax(),
        }
    }
}

// ── expressions ─────────────────────────────────────────────────

ast_node! {
    /// A `let name [: Type] = value in body` expression.
    LetExpr => LET_EXPR
}

impl LetExpr {
    /// The bound name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }

    /// The bound value (after `=`).
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::EQ)
    }

    /// The body the binding is in scope for (after `in`).
    #[must_use]
    pub fn body(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::IN_KW)
    }
}

ast_node! {
    /// A lambda expression (`λx y → body`).
    LambdaExpr => LAMBDA_EXPR
}

impl LambdaExpr {
    /// The parameter names, in order.
    pub fn param_names(&self) -> impl Iterator<Item = &str> {
        self.0
            .children_with_tokens()
            .take_while(|e| element_kind(*e) != SyntaxKind::ARROW)
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT)
            .map(|t| t.text())
    }

    /// The body expression (after `→`).
    #[must_use]
    pub fn body(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::ARROW)
    }
}

ast_node! {
    /// An `if cond then a else b` expression.
    IfExpr => IF_EXPR
}

impl IfExpr {
    /// The condition (after `if`).
    #[must_use]
    pub fn condition(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::IF_KW)
    }

    /// The consequent (after `then`).
    #[must_use]
    pub fn then_branch(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::THEN_KW)
    }

    /// The alternative (after `else`).
    #[must_use]
    pub fn else_branch(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::ELSE_KW)
    }
}

ast_node! {
    /// A `match scrutinee { arms }` expression.
    MatchExpr => MATCH_EXPR
}

impl MatchExpr {
    /// The scrutinee (between `match` and `{`).
    #[must_use]
    pub fn scrutinee(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::MATCH_KW)
    }

    /// The match arms, in order.
    pub fn arms(&self) -> impl Iterator<Item = MatchArm> + '_ {
        children(&self.0)
    }
}

ast_node! {
    /// A `handle { arms } in body` expression.
    HandleBlock => HANDLE_EXPR
}

impl HandleBlock {
    /// The effect-handler arms, in order.
    pub fn arms(&self) -> impl Iterator<Item = HandleArm> + '_ {
        children(&self.0)
    }

    /// The handled body (after `in`).
    #[must_use]
    pub fn body(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::IN_KW)
    }
}

ast_node! {
    /// A binary operator expression (`a + b`).
    BinOpExpr => BIN_EXPR
}

impl BinOpExpr {
    /// The left operand.
    #[must_use]
    pub fn lhs(&self) -> Option<Expr> {
        exprs(&self.0).next()
    }

    /// The right operand.
    #[must_use]
    pub fn rhs(&self) -> Option<Expr> {
        exprs(&self.0).nth(1)
    }

    /// The operator's source text (e.g. `+`).
    #[must_use]
    pub fn op(&self) -> Option<&str> {
        self.0
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| is_binop(t.kind()))
            .map(|t| t.text())
    }
}

ast_node! {
    /// A function application of a single argument (`f x`).
    AppExpr => APP_EXPR
}

impl AppExpr {
    /// The applied function.
    #[must_use]
    pub fn function(&self) -> Option<Expr> {
        exprs(&self.0).next()
    }

    /// The argument.
    #[must_use]
    pub fn argument(&self) -> Option<Expr> {
        exprs(&self.0).nth(1)
    }
}

ast_node! {
    /// A field access (`expr.field`).
    FieldExpr => FIELD_EXPR
}

impl FieldExpr {
    /// The receiver expression.
    #[must_use]
    pub fn receiver(&self) -> Option<Expr> {
        first_expr(&self.0)
    }

    /// The accessed field name (after `.`).
    #[must_use]
    pub fn field(&self) -> Option<&str> {
        self.0
            .children_with_tokens()
            .skip_while(|e| element_kind(*e) != SyntaxKind::DOT)
            .skip(1)
            .filter_map(|e| e.into_token())
            .find(|t| t.kind() == SyntaxKind::IDENT)
            .map(|t| t.text())
    }
}

ast_node! {
    /// A record literal (`{ x: 1, y: 2 }`).
    RecordLit => RECORD_LIT
}

impl RecordLit {
    /// The record fields, in order.
    pub fn fields(&self) -> impl Iterator<Item = RecordField> + '_ {
        children(&self.0)
    }
}

ast_node! {
    /// A tuple literal (`(a, b)`), including unit (`()`).
    TupleLit => TUPLE_LIT
}

impl TupleLit {
    /// The tuple elements, in order.
    pub fn elements(&self) -> impl Iterator<Item = Expr> + '_ {
        exprs(&self.0)
    }
}

ast_node! {
    /// A list literal (`[a, b, c]`).
    ListLit => LIST_LIT
}

impl ListLit {
    /// The list elements, in order.
    pub fn elements(&self) -> impl Iterator<Item = Expr> + '_ {
        exprs(&self.0)
    }
}

ast_node! {
    /// A parenthesised expression (`(e)`).
    ParenExpr => PAREN_EXPR
}

impl ParenExpr {
    /// The wrapped expression.
    #[must_use]
    pub fn inner(&self) -> Option<Expr> {
        first_expr(&self.0)
    }
}

/// A literal operand (integer, float, or string token).
#[derive(Debug, Clone)]
pub struct Literal(SyntaxToken);

impl Literal {
    /// The literal's source text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.0.text()
    }

    /// The kind of literal token.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.0.kind()
    }
}

/// A reference to a name used as an expression (a bare identifier).
#[derive(Debug, Clone)]
pub struct NameRef(SyntaxToken);

impl NameRef {
    /// The referenced name.
    #[must_use]
    pub fn text(&self) -> &str {
        self.0.text()
    }
}

/// Any expression.
///
/// Atomic operands (literals and bare names) are tokens rather than nodes, so
/// they appear as the [`Literal`](Expr::Literal) and [`Name`](Expr::Name)
/// variants instead of as [`AstNode`] types.
#[derive(Debug, Clone)]
pub enum Expr {
    /// `let .. in ..`
    Let(LetExpr),
    /// `λ.. → ..`
    Lambda(LambdaExpr),
    /// `if .. then .. else ..`
    If(IfExpr),
    /// `match .. { .. }`
    Match(MatchExpr),
    /// `handle { .. } in ..`
    Handle(HandleBlock),
    /// `a ⊕ b`
    BinOp(BinOpExpr),
    /// `f x`
    App(AppExpr),
    /// `e.field`
    Field(FieldExpr),
    /// `{ .. }`
    Record(RecordLit),
    /// `(.., ..)`
    Tuple(TupleLit),
    /// `[.., ..]`
    List(ListLit),
    /// `(e)`
    Paren(ParenExpr),
    /// An integer, float, or string literal.
    Literal(Literal),
    /// A bare name used as a value.
    Name(NameRef),
}

impl Expr {
    fn cast_node(node: SyntaxNode) -> Option<Self> {
        let expr = match node.kind() {
            SyntaxKind::LET_EXPR => Self::Let(LetExpr(node)),
            SyntaxKind::LAMBDA_EXPR => Self::Lambda(LambdaExpr(node)),
            SyntaxKind::IF_EXPR => Self::If(IfExpr(node)),
            SyntaxKind::MATCH_EXPR => Self::Match(MatchExpr(node)),
            SyntaxKind::HANDLE_EXPR => Self::Handle(HandleBlock(node)),
            SyntaxKind::BIN_EXPR => Self::BinOp(BinOpExpr(node)),
            SyntaxKind::APP_EXPR => Self::App(AppExpr(node)),
            SyntaxKind::FIELD_EXPR => Self::Field(FieldExpr(node)),
            SyntaxKind::RECORD_LIT => Self::Record(RecordLit(node)),
            SyntaxKind::TUPLE_LIT => Self::Tuple(TupleLit(node)),
            SyntaxKind::LIST_LIT => Self::List(ListLit(node)),
            SyntaxKind::PAREN_EXPR => Self::Paren(ParenExpr(node)),
            _ => return None,
        };
        Some(expr)
    }

    fn cast_token(tok: SyntaxToken) -> Option<Self> {
        match tok.kind() {
            SyntaxKind::INT | SyntaxKind::FLOAT | SyntaxKind::STRING => {
                Some(Self::Literal(Literal(tok)))
            }
            SyntaxKind::IDENT => Some(Self::Name(NameRef(tok))),
            _ => None,
        }
    }

    fn cast_element(element: ResolvedElementRef<'_, SyntaxKind>) -> Option<Self> {
        match element {
            NodeOrToken::Node(n) => Self::cast_node(n.clone()),
            NodeOrToken::Token(t) => Self::cast_token(t.clone()),
        }
    }

    /// The underlying CST node, or `None` for atomic operands ([`Literal`] and
    /// [`Name`](Self::Name)), which are tokens.
    #[must_use]
    pub fn syntax(&self) -> Option<&SyntaxNode> {
        match self {
            Self::Let(n) => Some(n.syntax()),
            Self::Lambda(n) => Some(n.syntax()),
            Self::If(n) => Some(n.syntax()),
            Self::Match(n) => Some(n.syntax()),
            Self::Handle(n) => Some(n.syntax()),
            Self::BinOp(n) => Some(n.syntax()),
            Self::App(n) => Some(n.syntax()),
            Self::Field(n) => Some(n.syntax()),
            Self::Record(n) => Some(n.syntax()),
            Self::Tuple(n) => Some(n.syntax()),
            Self::List(n) => Some(n.syntax()),
            Self::Paren(n) => Some(n.syntax()),
            Self::Literal(_) | Self::Name(_) => None,
        }
    }
}

// ── structural children ─────────────────────────────────────────

ast_node! {
    /// A function parameter (`name: Type`).
    Param => PARAM
}

impl Param {
    /// The parameter name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }
}

ast_node! {
    /// An ADT constructor (`Name(Field, ..)`).
    Constructor => CONSTRUCTOR
}

impl Constructor {
    /// The constructor name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }
}

ast_node! {
    /// A match arm (`pattern → body`). The pattern is reachable via
    /// [`AstNode::syntax`].
    MatchArm => MATCH_ARM
}

impl MatchArm {
    /// The arm body (after `→`).
    #[must_use]
    pub fn body(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::ARROW)
    }
}

ast_node! {
    /// A handle arm (`Effect → handler`). The effect is reachable via
    /// [`AstNode::syntax`].
    HandleArm => HANDLE_ARM
}

impl HandleArm {
    /// The handler implementation (after `→`).
    #[must_use]
    pub fn handler(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::ARROW)
    }
}

ast_node! {
    /// A record field (`name: value`).
    RecordField => RECORD_FIELD
}

impl RecordField {
    /// The field name. May be a keyword spelling (e.g. `actor`).
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.0
            .children_with_tokens()
            .take_while(|e| element_kind(*e) != SyntaxKind::COLON)
            .filter_map(|e| e.into_token())
            .find(|t| !is_trivia(t.kind()))
            .map(|t| t.text())
    }

    /// The field value (after `:`).
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::COLON)
    }
}

ast_node! {
    /// A qualified path (`Foo::Bar::Baz`).
    Path => PATH
}

impl Path {
    /// The path segments, in order.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT)
            .map(|t| t.text())
    }
}

/// The [`Param`] children of a declaration's `PARAM_LIST`, in order.
fn params(node: &SyntaxNode) -> impl Iterator<Item = Param> + '_ {
    node.children()
        .filter(|c| c.kind() == SyntaxKind::PARAM_LIST)
        .flat_map(|list| list.children())
        .filter_map(|c| Param::cast(c.clone()))
}

// ── token-kind predicates ───────────────────────────────────────

fn is_binop(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PLUS
            | SyntaxKind::MINUS
            | SyntaxKind::STAR
            | SyntaxKind::SLASH
            | SyntaxKind::LT
            | SyntaxKind::LE
            | SyntaxKind::GT
            | SyntaxKind::GE
            | SyntaxKind::EQ_EQ
            | SyntaxKind::BANG_EQ
    )
}

fn is_trivia(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
    )
}

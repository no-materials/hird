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
//! The projection covers declarations, expressions, type expressions, and
//! patterns. Supervisor bodies remain unprojected; reach their contents
//! through [`AstNode::syntax`].
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

/// Whether `node` carries an `opaque` modifier. Only ever set alongside `pub`,
/// since the parser rejects `opaque` without it.
fn is_opaque(node: &SyntaxNode) -> bool {
    token(node, SyntaxKind::OPAQUE_KW).is_some()
}

/// The kind of a node-or-token element.
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

/// The first type operand among a node's children/tokens.
fn first_type(node: &SyntaxNode) -> Option<TypeExpr> {
    node.children_with_tokens().find_map(TypeExpr::cast_element)
}

/// Every type operand among a node's children/tokens, in source order.
fn types(node: &SyntaxNode) -> impl Iterator<Item = TypeExpr> + '_ {
    node.children_with_tokens()
        .filter_map(TypeExpr::cast_element)
}

/// The first type operand appearing after the token `kw`.
fn type_after(node: &SyntaxNode, kw: SyntaxKind) -> Option<TypeExpr> {
    node.children_with_tokens()
        .skip_while(|e| element_kind(*e) != kw)
        .skip(1)
        .find_map(TypeExpr::cast_element)
}

/// The first type inside a node's `wrapper` child (`→ Type`, `: Type`).
fn type_in(node: &SyntaxNode, wrapper: SyntaxKind) -> Option<TypeExpr> {
    let w = node.children().find(|c| c.kind() == wrapper)?;
    first_type(w)
}

/// Every type operand inside a node's `wrapper` list child, in source order.
fn types_in(node: &SyntaxNode, wrapper: SyntaxKind) -> impl Iterator<Item = TypeExpr> + '_ {
    node.children()
        .filter(move |c| c.kind() == wrapper)
        .flat_map(|list| list.children_with_tokens())
        .filter_map(TypeExpr::cast_element)
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
    /// A use import (`use A.B`, `use M as N`, or `use M.{ a, b }`).
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
        // next non-trivia token after it. (Path segments and selective-group
        // members are nested in child nodes, not direct children here.)
        self.0
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .filter(|t| !is_trivia(t.kind()))
            .skip_while(|t| t.text() != "as")
            .nth(1)
            .map(|t| t.text())
    }

    /// The member names of a selective group (`.{ a, b }`), in source order.
    /// Empty for whole-module and aliased imports.
    pub fn selected(&self) -> impl Iterator<Item = &str> {
        self.selected_tokens().map(|t| t.text())
    }

    /// The member-name tokens of a selective group, in source order — the
    /// span-bearing form of [`selected`](Self::selected).
    pub fn selected_tokens(&self) -> impl Iterator<Item = &SyntaxToken> {
        self.0
            .children()
            .filter(|c| c.kind() == SyntaxKind::USE_GROUP)
            .flat_map(|group| group.children_with_tokens())
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT)
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

    /// The declared return type (after `→`), if annotated.
    #[must_use]
    pub fn return_type(&self) -> Option<TypeExpr> {
        type_in(&self.0, SyntaxKind::RETURN_TYPE)
    }

    /// The body expression (after `=`).
    #[must_use]
    pub fn body(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::EQ)
    }

    /// The declared effect-row annotation (`! { … }`), if present. This is the
    /// function's own row; annotations nested inside parameter types are
    /// reached through those types.
    #[must_use]
    pub fn effect_ann(&self) -> Option<EffectAnn> {
        child(&self.0)
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

    /// Whether the type is opaque (`pub opaque type`): its name is exported but
    /// its constructors stay module-private. Implies [`is_pub`](Self::is_pub).
    #[must_use]
    pub fn is_opaque(&self) -> bool {
        is_opaque(&self.0)
    }

    /// The type name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }

    /// The type parameter names (`<A, B>`), in order.
    pub fn type_params(&self) -> impl Iterator<Item = &str> {
        self.0
            .children()
            .filter(|c| c.kind() == SyntaxKind::TYPE_PARAMS)
            .flat_map(|list| list.children_with_tokens())
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT)
            .map(|t| t.text())
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

    /// The type parameter names (`<a, b>`), in order.
    pub fn type_params(&self) -> impl Iterator<Item = &str> {
        self.0
            .children()
            .filter(|c| c.kind() == SyntaxKind::TYPE_PARAMS)
            .flat_map(|list| list.children_with_tokens())
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT)
            .map(|t| t.text())
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

    /// The type parameter names (`<t, u>`), in order.
    pub fn type_params(&self) -> impl Iterator<Item = &str> {
        self.0
            .children()
            .filter(|c| c.kind() == SyntaxKind::TYPE_PARAMS)
            .flat_map(|list| list.children_with_tokens())
            .filter_map(|e| e.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT)
            .map(|t| t.text())
    }

    /// The input (argument) type, between `:` and `→`.
    #[must_use]
    pub fn input(&self) -> Option<TypeExpr> {
        type_after(&self.0, SyntaxKind::COLON)
    }

    /// The output (result) type, after `→`.
    #[must_use]
    pub fn output(&self) -> Option<TypeExpr> {
        type_after(&self.0, SyntaxKind::ARROW)
    }

    /// The trailing effect-row annotation (`! { … }`), if present. Unioned into
    /// the generated function's row alongside the tool's own effect.
    #[must_use]
    pub fn effect_ann(&self) -> Option<EffectAnn> {
        child(&self.0)
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

    /// The declared return type (after `→`), if annotated.
    #[must_use]
    pub fn return_type(&self) -> Option<TypeExpr> {
        type_in(&self.0, SyntaxKind::RETURN_TYPE)
    }
}

ast_node! {
    /// An actor declaration (`actor Name { members } ! {row}`).
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

    /// The named body fields (`state`, `message`, `init`), in source order.
    pub fn fields(&self) -> impl Iterator<Item = ActorField> + '_ {
        children(&self.0)
    }

    /// The `handle` clauses, in source order.
    pub fn handlers(&self) -> impl Iterator<Item = ActorHandler> + '_ {
        children(&self.0)
    }

    /// The trailing effect summary (`! { … }` after the body), if declared.
    #[must_use]
    pub fn effect_ann(&self) -> Option<EffectAnn> {
        child(&self.0)
    }
}

ast_node! {
    /// A named actor body field (`name: value`). The value is a function
    /// signature with a body (`init`), a type with an ADT tail (`message`), or
    /// a plain type (`state`).
    ActorField => ACTOR_FIELD
}

impl ActorField {
    /// The field name (`state`, `message`, or `init`).
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }

    /// The field's type value (after `:`, before any `=`): the state type, or
    /// the message type name. `None` when the value is a function signature.
    #[must_use]
    pub fn ty(&self) -> Option<TypeExpr> {
        self.0
            .children_with_tokens()
            .skip_while(|e| element_kind(*e) != SyntaxKind::COLON)
            .skip(1)
            .take_while(|e| element_kind(*e) != SyntaxKind::EQ)
            .find_map(TypeExpr::cast_element)
    }

    /// The function signature of an `init` field.
    #[must_use]
    pub fn fn_sig(&self) -> Option<FnSig> {
        child(&self.0)
    }

    /// The message constructors (after `=`), in source order. Empty for
    /// non-`message` fields.
    pub fn constructors(&self) -> impl Iterator<Item = Constructor> + '_ {
        children(&self.0)
    }

    /// The body expression of an `init` field (after `=`).
    #[must_use]
    pub fn body(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::EQ)
    }
}

ast_node! {
    /// An actor message handler
    /// (`handle Pattern, State → Type ! { … } = body`).
    ActorHandler => ACTOR_HANDLER
}

impl ActorHandler {
    /// The message pattern (a constructor of the actor's message type).
    #[must_use]
    pub fn message_pattern(&self) -> Option<Pattern> {
        children(&self.0).next()
    }

    /// The current-state pattern (the trailing comma-separated pattern).
    #[must_use]
    pub fn state_pattern(&self) -> Option<Pattern> {
        children::<Pattern>(&self.0).nth(1)
    }

    /// The declared return type (after `→`), if annotated.
    #[must_use]
    pub fn return_type(&self) -> Option<TypeExpr> {
        type_in(&self.0, SyntaxKind::RETURN_TYPE)
    }

    /// The handler's effect-row annotation (`! { … }`), if present.
    #[must_use]
    pub fn effect_ann(&self) -> Option<EffectAnn> {
        child(&self.0)
    }

    /// The body expression (after `=`).
    #[must_use]
    pub fn body(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::EQ)
    }
}

ast_node! {
    /// An unnamed function signature (`fn(params) → Ret ! { … }`), the value
    /// of an actor `init` field.
    FnSig => FN_SIG
}

impl FnSig {
    /// The declared parameters, in order.
    pub fn params(&self) -> impl Iterator<Item = Param> + '_ {
        params(&self.0)
    }

    /// The declared return type (after `→`), if annotated.
    #[must_use]
    pub fn return_type(&self) -> Option<TypeExpr> {
        type_in(&self.0, SyntaxKind::RETURN_TYPE)
    }

    /// The declared effect-row annotation (`! { … }`), if present.
    #[must_use]
    pub fn effect_ann(&self) -> Option<EffectAnn> {
        child(&self.0)
    }
}

ast_node! {
    /// A supervisor declaration (`supervisor Name { fields }`). The body is a
    /// closed set of `strategy`, `intensity`, `period`, and `children` fields.
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

    /// The body fields (`strategy`, `intensity`, `period`, `children`), in
    /// source order.
    pub fn fields(&self) -> impl Iterator<Item = SupervisorField> + '_ {
        children(&self.0)
    }
}

ast_node! {
    /// A supervisor body field (`name: value`). The value is an expression:
    /// a strategy or restart identifier, an integer literal, or the `children`
    /// list of child-spec records.
    SupervisorField => SUPERVISOR_FIELD
}

impl SupervisorField {
    /// The field name (`strategy`, `intensity`, `period`, or `children`).
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }

    /// The field value (after `:`).
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::COLON)
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

    /// The type annotation (`: Type`), if present.
    #[must_use]
    pub fn annotation(&self) -> Option<TypeExpr> {
        type_in(&self.0, SyntaxKind::TYPE_ANN)
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
    /// A `spawn(Actor, args…)` expression. The actor name is a namespace
    /// reference, not an expression.
    SpawnExpr => SPAWN_EXPR
}

impl SpawnExpr {
    /// The spawned actor's name token, for span-bearing diagnostics.
    #[must_use]
    pub fn actor_token(&self) -> Option<&SyntaxToken> {
        token(&self.0, SyntaxKind::IDENT)
    }

    /// The spawned actor's name.
    #[must_use]
    pub fn actor_name(&self) -> Option<&str> {
        self.actor_token().map(|t| t.text())
    }

    /// The init arguments (everything after the actor name), in order.
    pub fn args(&self) -> impl Iterator<Item = Expr> + '_ {
        self.0
            .children_with_tokens()
            .skip_while(|e| element_kind(*e) != SyntaxKind::COMMA)
            .filter_map(Expr::cast_element)
    }
}

ast_node! {
    /// A `send(pid, msg)` expression: fire-and-forget message delivery.
    SendExpr => SEND_EXPR
}

impl SendExpr {
    /// The destination expression (a `Pid<Msg>`).
    #[must_use]
    pub fn pid(&self) -> Option<Expr> {
        exprs(&self.0).next()
    }

    /// The message expression.
    #[must_use]
    pub fn message(&self) -> Option<Expr> {
        exprs(&self.0).nth(1)
    }
}

ast_node! {
    /// A `request(pid, ctor)` expression: send with an embedded reply channel,
    /// awaiting the reply.
    RequestExpr => REQUEST_EXPR
}

impl RequestExpr {
    /// The destination expression (a `Pid<Msg>`).
    #[must_use]
    pub fn pid(&self) -> Option<Expr> {
        exprs(&self.0).next()
    }

    /// The message-building expression (`ReplyTo<T> → Msg`), typically a
    /// message constructor.
    #[must_use]
    pub fn message_fn(&self) -> Option<Expr> {
        exprs(&self.0).nth(1)
    }
}

ast_node! {
    /// A `reply(reply_to, value)` expression: answers a request on its typed
    /// reply channel.
    ReplyExpr => REPLY_EXPR
}

impl ReplyExpr {
    /// The reply channel expression (a `ReplyTo<T>`).
    #[must_use]
    pub fn reply_to(&self) -> Option<Expr> {
        exprs(&self.0).next()
    }

    /// The replied value expression.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        exprs(&self.0).nth(1)
    }
}

ast_node! {
    /// A `crash!(message)` (or `panic!(message)`) expression: the divergent
    /// primitive that terminates the process. It never returns, so it fits any
    /// result context.
    CrashExpr => CRASH_EXPR
}

impl CrashExpr {
    /// The crash message expression (a `String`).
    #[must_use]
    pub fn message(&self) -> Option<Expr> {
        first_expr(&self.0)
    }

    /// Whether the primitive was spelled `panic!` rather than `crash!`. Both
    /// are aliases with identical semantics; the spelling is preserved only for
    /// diagnostics and faithful rendering.
    #[must_use]
    pub fn is_panic(&self) -> bool {
        token(&self.0, SyntaxKind::PANIC_KW).is_some()
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

    /// The underlying CST token.
    #[must_use]
    pub fn syntax(&self) -> &SyntaxToken {
        &self.0
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

    /// The underlying CST token.
    #[must_use]
    pub fn syntax(&self) -> &SyntaxToken {
        &self.0
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
    /// `spawn(Actor, ..)`
    Spawn(SpawnExpr),
    /// `send(pid, msg)`
    Send(SendExpr),
    /// `request(pid, ctor)`
    Request(RequestExpr),
    /// `reply(reply_to, value)`
    Reply(ReplyExpr),
    /// `crash!(msg)` / `panic!(msg)`
    Crash(CrashExpr),
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
    /// Casts a node to its matching `Expr` variant, or `None`.
    fn cast_node(node: SyntaxNode) -> Option<Self> {
        let expr = match node.kind() {
            SyntaxKind::LET_EXPR => Self::Let(LetExpr(node)),
            SyntaxKind::LAMBDA_EXPR => Self::Lambda(LambdaExpr(node)),
            SyntaxKind::IF_EXPR => Self::If(IfExpr(node)),
            SyntaxKind::MATCH_EXPR => Self::Match(MatchExpr(node)),
            SyntaxKind::HANDLE_EXPR => Self::Handle(HandleBlock(node)),
            SyntaxKind::SPAWN_EXPR => Self::Spawn(SpawnExpr(node)),
            SyntaxKind::SEND_EXPR => Self::Send(SendExpr(node)),
            SyntaxKind::REQUEST_EXPR => Self::Request(RequestExpr(node)),
            SyntaxKind::REPLY_EXPR => Self::Reply(ReplyExpr(node)),
            SyntaxKind::CRASH_EXPR => Self::Crash(CrashExpr(node)),
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

    /// Casts a token to its matching `Expr` variant — a literal or name — or
    /// `None`.
    fn cast_token(tok: SyntaxToken) -> Option<Self> {
        match tok.kind() {
            SyntaxKind::INT | SyntaxKind::FLOAT | SyntaxKind::STRING => {
                Some(Self::Literal(Literal(tok)))
            }
            SyntaxKind::IDENT => Some(Self::Name(NameRef(tok))),
            _ => None,
        }
    }

    /// Casts a node-or-token element to its matching `Expr` variant, or `None`.
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
            Self::Spawn(n) => Some(n.syntax()),
            Self::Send(n) => Some(n.syntax()),
            Self::Request(n) => Some(n.syntax()),
            Self::Reply(n) => Some(n.syntax()),
            Self::Crash(n) => Some(n.syntax()),
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

// ── type expressions ────────────────────────────────────────────

ast_node! {
    /// An applied type (`List<Int>`).
    AppType => APP_TYPE
}

impl AppType {
    /// The applied constructor name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }

    /// The type arguments (`<..>`), in order.
    pub fn args(&self) -> impl Iterator<Item = TypeExpr> + '_ {
        types_in(&self.0, SyntaxKind::TYPE_ARGS)
    }
}

ast_node! {
    /// A function type (`A → B`), optionally carrying an effect-row annotation
    /// (`A → B ! { … }`).
    FnType => FN_TYPE
}

impl FnType {
    /// The parameter types: every operand but the last.
    pub fn params(&self) -> impl Iterator<Item = TypeExpr> + '_ {
        // Lag by one: hold each operand back until the next arrives, so the
        // final operand (the result type) is never yielded.
        let mut prev = None;
        types(&self.0).filter_map(move |t| prev.replace(t))
    }

    /// The result type: the final operand.
    #[must_use]
    pub fn return_type(&self) -> Option<TypeExpr> {
        types(&self.0).last()
    }

    /// The effect-row annotation on the arrow (`A → B ! { … }`), if present.
    #[must_use]
    pub fn effect_ann(&self) -> Option<EffectAnn> {
        child(&self.0)
    }
}

ast_node! {
    /// A tuple type (`(A, B)`), including unit (`()`).
    TupleType => TUPLE_TYPE
}

impl TupleType {
    /// The element types, in order.
    pub fn elements(&self) -> impl Iterator<Item = TypeExpr> + '_ {
        types(&self.0)
    }
}

ast_node! {
    /// A record type (`{ name: Type, … }`).
    RecordType => RECORD_TYPE
}

impl RecordType {
    /// The record's fields, in order.
    pub fn fields(&self) -> impl Iterator<Item = RecordTypeField> + '_ {
        children(&self.0)
    }
}

ast_node! {
    /// A record-type field (`name: Type`).
    RecordTypeField => RECORD_TYPE_FIELD
}

impl RecordTypeField {
    /// The field name. May be a keyword spelling (e.g. `tool`).
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.0
            .children_with_tokens()
            .take_while(|e| element_kind(*e) != SyntaxKind::COLON)
            .filter_map(|e| e.into_token())
            .find(|t| !is_trivia(t.kind()))
            .map(|t| t.text())
    }

    /// The field type (after `:`).
    #[must_use]
    pub fn ty(&self) -> Option<TypeExpr> {
        type_after(&self.0, SyntaxKind::COLON)
    }
}

ast_node! {
    /// A parenthesised type (`(T)`).
    ParenType => PAREN_TYPE
}

impl ParenType {
    /// The wrapped type.
    #[must_use]
    pub fn inner(&self) -> Option<TypeExpr> {
        first_type(&self.0)
    }
}

/// A bare type name: a named type (`Int`) or a type variable (`a`). The two are
/// indistinguishable here; the checker classifies them.
#[derive(Debug, Clone)]
pub struct NameType(SyntaxToken);

impl NameType {
    /// The name's source text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.0.text()
    }

    /// The underlying CST token.
    #[must_use]
    pub fn syntax(&self) -> &SyntaxToken {
        &self.0
    }
}

/// Any type expression.
///
/// A bare name ([`Name`](Self::Name)) is a token rather than a node, mirroring
/// the atomic operands of [`Expr`].
#[derive(Debug, Clone)]
pub enum TypeExpr {
    /// `C<..>`
    App(AppType),
    /// `A → B`
    Fn(FnType),
    /// `(.., ..)`
    Tuple(TupleType),
    /// `{ .., .. }`
    Record(RecordType),
    /// `(T)`
    Paren(ParenType),
    /// A named type or type variable.
    Name(NameType),
}

impl TypeExpr {
    /// Casts a node to its matching `TypeExpr` variant, or `None`.
    fn cast_node(node: SyntaxNode) -> Option<Self> {
        let ty = match node.kind() {
            SyntaxKind::APP_TYPE => Self::App(AppType(node)),
            SyntaxKind::FN_TYPE => Self::Fn(FnType(node)),
            SyntaxKind::TUPLE_TYPE => Self::Tuple(TupleType(node)),
            SyntaxKind::RECORD_TYPE => Self::Record(RecordType(node)),
            SyntaxKind::PAREN_TYPE => Self::Paren(ParenType(node)),
            _ => return None,
        };
        Some(ty)
    }

    /// Casts a token to its matching `TypeExpr` variant — a bare name — or
    /// `None`.
    fn cast_token(tok: SyntaxToken) -> Option<Self> {
        match tok.kind() {
            SyntaxKind::IDENT => Some(Self::Name(NameType(tok))),
            _ => None,
        }
    }

    /// Casts a node-or-token element to its matching `TypeExpr` variant, or
    /// `None`.
    fn cast_element(element: ResolvedElementRef<'_, SyntaxKind>) -> Option<Self> {
        match element {
            NodeOrToken::Node(n) => Self::cast_node(n.clone()),
            NodeOrToken::Token(t) => Self::cast_token(t.clone()),
        }
    }

    /// The underlying CST node, or `None` for a bare [`Name`](Self::Name), which
    /// is a token.
    #[must_use]
    pub fn syntax(&self) -> Option<&SyntaxNode> {
        match self {
            Self::App(n) => Some(n.syntax()),
            Self::Fn(n) => Some(n.syntax()),
            Self::Tuple(n) => Some(n.syntax()),
            Self::Record(n) => Some(n.syntax()),
            Self::Paren(n) => Some(n.syntax()),
            Self::Name(_) => None,
        }
    }
}

// ── effect annotations ───────────────────────────────────────────

ast_node! {
    /// An effect-row annotation (`! { E1, E2 }`).
    EffectAnn => EFFECT_ANN
}

impl EffectAnn {
    /// The annotated effects, in order. Each is a type expression: a bare
    /// lowercase name is a row variable, a `PascalCase` name or application is
    /// an effect (`Log`, `Tool<X>`). The checker classifies them.
    pub fn effects(&self) -> impl Iterator<Item = TypeExpr> + '_ {
        types(&self.0)
    }
}

// ── patterns ─────────────────────────────────────────────────────

ast_node! {
    /// A constructor pattern (`Foo(a, b)` or nullary `Foo`).
    ConstructorPat => CONSTRUCTOR_PAT
}

impl ConstructorPat {
    /// The constructor name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }

    /// The sub-pattern for each constructor field, in order.
    pub fn fields(&self) -> impl Iterator<Item = Pattern> + '_ {
        children(&self.0)
    }
}

ast_node! {
    /// A tuple pattern (`(a, b)`), including the empty pattern (`()`).
    TuplePat => TUPLE_PAT
}

impl TuplePat {
    /// The element patterns, in order.
    pub fn elements(&self) -> impl Iterator<Item = Pattern> + '_ {
        children(&self.0)
    }
}

ast_node! {
    /// A literal pattern (`1`, `"hi"`).
    LiteralPat => LITERAL_PAT
}

impl LiteralPat {
    /// The matched literal (integer, float, or string).
    #[must_use]
    pub fn literal(&self) -> Option<Literal> {
        self.0
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| {
                matches!(
                    t.kind(),
                    SyntaxKind::INT | SyntaxKind::FLOAT | SyntaxKind::STRING
                )
            })
            .map(|t| Literal(t.clone()))
    }
}

ast_node! {
    /// A wildcard pattern (`_`).
    WildcardPat => WILDCARD_PAT
}

ast_node! {
    /// A variable binding pattern (`x`).
    BindPat => BIND_PAT
}

impl BindPat {
    /// The bound name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        name(&self.0)
    }
}

/// Any pattern.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// `Foo(..)`
    Constructor(ConstructorPat),
    /// `(.., ..)`
    Tuple(TuplePat),
    /// `1`, `"hi"`
    Literal(LiteralPat),
    /// `_`
    Wildcard(WildcardPat),
    /// `x`
    Bind(BindPat),
}

impl AstNode for Pattern {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::CONSTRUCTOR_PAT
                | SyntaxKind::TUPLE_PAT
                | SyntaxKind::LITERAL_PAT
                | SyntaxKind::WILDCARD_PAT
                | SyntaxKind::BIND_PAT
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        let pat = match syntax.kind() {
            SyntaxKind::CONSTRUCTOR_PAT => Self::Constructor(ConstructorPat(syntax)),
            SyntaxKind::TUPLE_PAT => Self::Tuple(TuplePat(syntax)),
            SyntaxKind::LITERAL_PAT => Self::Literal(LiteralPat(syntax)),
            SyntaxKind::WILDCARD_PAT => Self::Wildcard(WildcardPat(syntax)),
            SyntaxKind::BIND_PAT => Self::Bind(BindPat(syntax)),
            _ => return None,
        };
        Some(pat)
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Constructor(n) => n.syntax(),
            Self::Tuple(n) => n.syntax(),
            Self::Literal(n) => n.syntax(),
            Self::Wildcard(n) => n.syntax(),
            Self::Bind(n) => n.syntax(),
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

    /// The declared type (after `:`).
    #[must_use]
    pub fn ty(&self) -> Option<TypeExpr> {
        type_after(&self.0, SyntaxKind::COLON)
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

    /// The field types, in order.
    pub fn fields(&self) -> impl Iterator<Item = TypeExpr> + '_ {
        types_in(&self.0, SyntaxKind::FIELD_LIST)
    }
}

ast_node! {
    /// A match arm (`pattern → body`).
    MatchArm => MATCH_ARM
}

impl MatchArm {
    /// The arm's pattern.
    #[must_use]
    pub fn pattern(&self) -> Option<Pattern> {
        child(&self.0)
    }

    /// The arm body (after `→`).
    #[must_use]
    pub fn body(&self) -> Option<Expr> {
        expr_after(&self.0, SyntaxKind::ARROW)
    }
}

ast_node! {
    /// A handle arm (`Effect → handler`).
    HandleArm => HANDLE_ARM
}

impl HandleArm {
    /// The handled effect head (before `→`), e.g. `Log` or `Tool<ReadRepo>`. A
    /// bare lowercase name is rejected by the checker; the effect is the arm's
    /// first type operand.
    #[must_use]
    pub fn effect(&self) -> Option<TypeExpr> {
        first_type(&self.0)
    }

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
    /// A dotted path (`Foo.Bar.Baz`).
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

/// Whether `kind` is a binary-operator token.
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
            | SyntaxKind::ANDAND
            | SyntaxKind::OROR
    )
}

/// Whether `kind` is trivia: whitespace or a comment.
fn is_trivia(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
    )
}

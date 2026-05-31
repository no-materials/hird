// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Flat syntax kind enum for the cstree-backed CST.
//!
//! Covers both token kinds (mapped from [`hird_lex::TokenKind`]) and
//! composite node kinds for grammar productions.

use cstree::RawSyntaxKind;
use hird_lex::{LexError, TokenKind};

/// Unified kind for every node and token in the Hird CST.
///
/// Token kinds mirror [`TokenKind`] but are flattened (no data-carrying
/// variants). Node kinds correspond to grammar productions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
#[expect(
    non_camel_case_types,
    reason = "SCREAMING_SNAKE is conventional for syntax kinds"
)]
pub enum SyntaxKind {
    // === Token kinds (mirrored from hird_lex::TokenKind) ===

    // -- Keywords --
    /// `let`
    LET_KW,
    /// `fn`
    FN_KW,
    /// `match`
    MATCH_KW,
    /// `type`
    TYPE_KW,
    /// `actor`
    ACTOR_KW,
    /// `supervisor`
    SUPERVISOR_KW,
    /// `effect`
    EFFECT_KW,
    /// `tool`
    TOOL_KW,
    /// `handle`
    HANDLE_KW,
    /// `spawn`
    SPAWN_KW,
    /// `send`
    SEND_KW,
    /// `request`
    REQUEST_KW,
    /// `use`
    USE_KW,
    /// `module`
    MODULE_KW,
    /// `pub`
    PUB_KW,
    /// `extern`
    EXTERN_KW,
    /// `if`
    IF_KW,
    /// `then`
    THEN_KW,
    /// `else`
    ELSE_KW,
    /// `in`
    IN_KW,

    // -- Identifiers and literals --
    /// Identifier token.
    IDENT,
    /// Integer literal.
    INT,
    /// Floating-point literal.
    FLOAT,
    /// String literal.
    STRING,

    // -- Operators --
    /// `+`
    PLUS,
    /// `-`
    MINUS,
    /// `*`
    STAR,
    /// `/`
    SLASH,
    /// `<`
    LT,
    /// `>`
    GT,
    /// `<=`
    LE,
    /// `>=`
    GE,
    /// `==`
    EQ_EQ,
    /// `!=`
    BANG_EQ,
    /// `=`
    EQ,
    /// `→`
    ARROW,
    /// `⇒`
    FAT_ARROW,
    /// `λ`
    LAMBDA,
    /// `|`
    PIPE,
    /// `!`
    BANG,
    /// `.`
    DOT,
    /// `:`
    COLON,
    /// `::`
    COLON_COLON,

    // -- Delimiters --
    /// `(`
    L_PAREN,
    /// `)`
    R_PAREN,
    /// `{`
    L_BRACE,
    /// `}`
    R_BRACE,
    /// `[`
    L_BRACKET,
    /// `]`
    R_BRACKET,
    /// `,`
    COMMA,
    /// `;`
    SEMICOLON,

    // -- Trivia --
    /// Line comment (`// ...`).
    LINE_COMMENT,
    /// Block comment (`/* ... */`).
    BLOCK_COMMENT,
    /// Whitespace (synthesised by the parser from lexer gaps).
    WHITESPACE,

    // -- Special --
    /// End of file marker.
    EOF,

    // -- Lexer errors (flattened from TokenKind::Error) --
    /// Unterminated string literal.
    ERR_UNTERMINATED_STRING,
    /// Unterminated block comment.
    ERR_UNTERMINATED_BLOCK_COMMENT,
    /// Unexpected character.
    ERR_UNEXPECTED_CHAR,
    /// Non-canonical identifier naming.
    ERR_NON_CANONICAL_NAME,

    // === Composite node kinds ===
    /// Root node wrapping an entire source file.
    SOURCE_FILE,
    /// Module declaration (`module Name`).
    MODULE_DECL,
    /// Use import (`use Path::Name`).
    USE_DECL,
    /// Module path (`Foo::Bar::Baz`).
    PATH,
    /// Function declaration.
    FN_DECL,
    /// Visibility modifier (`pub`).
    VISIBILITY,
    /// Parameter list in function signature.
    PARAM_LIST,
    /// Single parameter (`name: Type`).
    PARAM,
    /// Return type annotation (`→ Type`).
    RETURN_TYPE,
    /// Effect annotation (`! { E1, E2 }`).
    EFFECT_ANN,
    /// Type declaration (ADT).
    TYPE_DECL,
    /// Type parameter list (`<A, B>`).
    TYPE_PARAMS,
    /// ADT constructor (`Foo(Bar, Baz)`).
    CONSTRUCTOR,
    /// Field list in a constructor or record.
    FIELD_LIST,
    /// Actor declaration.
    ACTOR_DECL,
    /// Actor body field.
    ACTOR_FIELD,
    /// Supervisor declaration.
    SUPERVISOR_DECL,
    /// Supervisor body field.
    SUPERVISOR_FIELD,
    /// Effect declaration.
    EFFECT_DECL,
    /// Tool declaration.
    TOOL_DECL,
    /// Extern function declaration.
    EXTERN_DECL,
    /// `let ... = ... in ...` expression.
    LET_EXPR,
    /// Lambda expression (`λx → ...`).
    LAMBDA_EXPR,
    /// Match expression.
    MATCH_EXPR,
    /// Single match arm (`pattern ⇒ expr`).
    MATCH_ARM,
    /// If-then-else expression.
    IF_EXPR,
    /// Handle expression.
    HANDLE_EXPR,
    /// Single handle arm (`Effect ⇒ impl`).
    HANDLE_ARM,
    /// Binary operator expression.
    BIN_EXPR,
    /// Function application (`f x y`).
    APP_EXPR,
    /// Field access (`expr.field`).
    FIELD_EXPR,
    /// Tuple literal (`(a, b, c)`).
    TUPLE_LIT,
    /// List literal (`[a, b, c]`).
    LIST_LIT,
    /// Record literal (`{ x: 1, y: 2 }`).
    RECORD_LIT,
    /// Record field (`name: expr`).
    RECORD_FIELD,
    /// Parenthesised expression.
    PAREN_EXPR,
    /// Type annotation (`: Type`).
    TYPE_ANN,
    /// Type argument list (`<A, B>`).
    TYPE_ARGS,
    /// Applied type (`List<Int>`).
    APP_TYPE,
    /// Function type (`A → B`).
    FN_TYPE,
    /// Parenthesised type (`(T)`).
    PAREN_TYPE,
    /// Tuple type (`(A, B)`).
    TUPLE_TYPE,
    /// Constructor pattern (`Foo(a, b)`).
    CONSTRUCTOR_PAT,
    /// Tuple pattern (`(a, b)`).
    TUPLE_PAT,
    /// Literal pattern.
    LITERAL_PAT,
    /// Wildcard pattern (`_`).
    WILDCARD_PAT,
    /// Variable binding pattern.
    BIND_PAT,
    /// Error recovery node wrapping unparseable tokens.
    ERROR,
}

impl From<TokenKind> for SyntaxKind {
    fn from(kind: TokenKind) -> Self {
        match kind {
            TokenKind::Let => Self::LET_KW,
            TokenKind::Fn => Self::FN_KW,
            TokenKind::Match => Self::MATCH_KW,
            TokenKind::Type => Self::TYPE_KW,
            TokenKind::Actor => Self::ACTOR_KW,
            TokenKind::Supervisor => Self::SUPERVISOR_KW,
            TokenKind::Effect => Self::EFFECT_KW,
            TokenKind::Tool => Self::TOOL_KW,
            TokenKind::Handle => Self::HANDLE_KW,
            TokenKind::Spawn => Self::SPAWN_KW,
            TokenKind::Send => Self::SEND_KW,
            TokenKind::Request => Self::REQUEST_KW,
            TokenKind::Use => Self::USE_KW,
            TokenKind::Module => Self::MODULE_KW,
            TokenKind::Pub => Self::PUB_KW,
            TokenKind::Extern => Self::EXTERN_KW,
            TokenKind::If => Self::IF_KW,
            TokenKind::Then => Self::THEN_KW,
            TokenKind::Else => Self::ELSE_KW,
            TokenKind::In => Self::IN_KW,
            TokenKind::Ident => Self::IDENT,
            TokenKind::Int => Self::INT,
            TokenKind::Float => Self::FLOAT,
            TokenKind::Str => Self::STRING,
            TokenKind::Plus => Self::PLUS,
            TokenKind::Minus => Self::MINUS,
            TokenKind::Star => Self::STAR,
            TokenKind::Slash => Self::SLASH,
            TokenKind::Lt => Self::LT,
            TokenKind::Gt => Self::GT,
            TokenKind::Le => Self::LE,
            TokenKind::Ge => Self::GE,
            TokenKind::EqEq => Self::EQ_EQ,
            TokenKind::BangEq => Self::BANG_EQ,
            TokenKind::Eq => Self::EQ,
            TokenKind::Arrow => Self::ARROW,
            TokenKind::FatArrow => Self::FAT_ARROW,
            TokenKind::Lambda => Self::LAMBDA,
            TokenKind::Pipe => Self::PIPE,
            TokenKind::Bang => Self::BANG,
            TokenKind::Dot => Self::DOT,
            TokenKind::Colon => Self::COLON,
            TokenKind::ColonColon => Self::COLON_COLON,
            TokenKind::LParen => Self::L_PAREN,
            TokenKind::RParen => Self::R_PAREN,
            TokenKind::LBrace => Self::L_BRACE,
            TokenKind::RBrace => Self::R_BRACE,
            TokenKind::LBracket => Self::L_BRACKET,
            TokenKind::RBracket => Self::R_BRACKET,
            TokenKind::Comma => Self::COMMA,
            TokenKind::Semicolon => Self::SEMICOLON,
            TokenKind::LineComment => Self::LINE_COMMENT,
            TokenKind::BlockComment => Self::BLOCK_COMMENT,
            TokenKind::Eof => Self::EOF,
            TokenKind::Error(err) => match err {
                LexError::UnterminatedString => Self::ERR_UNTERMINATED_STRING,
                LexError::UnterminatedBlockComment => Self::ERR_UNTERMINATED_BLOCK_COMMENT,
                LexError::UnexpectedChar => Self::ERR_UNEXPECTED_CHAR,
                LexError::NonCanonicalName => Self::ERR_NON_CANONICAL_NAME,
            },
        }
    }
}

impl cstree::Syntax for SyntaxKind {
    fn from_raw(raw: RawSyntaxKind) -> Self {
        assert!(
            raw.0 <= Self::ERROR as u32,
            "invalid SyntaxKind discriminant: {}",
            raw.0
        );
        #[expect(unsafe_code, reason = "repr(u32) enum from checked discriminant")]
        // SAFETY: SyntaxKind is #[repr(u32)] with contiguous discriminants
        // starting at 0. The assert above guarantees the value is in range.
        unsafe {
            core::mem::transmute::<u32, Self>(raw.0)
        }
    }

    fn into_raw(self) -> RawSyntaxKind {
        RawSyntaxKind(self as u32)
    }

    fn static_text(self) -> Option<&'static str> {
        match self {
            Self::LET_KW => Some("let"),
            Self::FN_KW => Some("fn"),
            Self::MATCH_KW => Some("match"),
            Self::TYPE_KW => Some("type"),
            Self::ACTOR_KW => Some("actor"),
            Self::SUPERVISOR_KW => Some("supervisor"),
            Self::EFFECT_KW => Some("effect"),
            Self::TOOL_KW => Some("tool"),
            Self::HANDLE_KW => Some("handle"),
            Self::SPAWN_KW => Some("spawn"),
            Self::SEND_KW => Some("send"),
            Self::REQUEST_KW => Some("request"),
            Self::USE_KW => Some("use"),
            Self::MODULE_KW => Some("module"),
            Self::PUB_KW => Some("pub"),
            Self::EXTERN_KW => Some("extern"),
            Self::IF_KW => Some("if"),
            Self::THEN_KW => Some("then"),
            Self::ELSE_KW => Some("else"),
            Self::IN_KW => Some("in"),
            Self::PLUS => Some("+"),
            Self::MINUS => Some("-"),
            Self::STAR => Some("*"),
            Self::SLASH => Some("/"),
            Self::LT => Some("<"),
            Self::GT => Some(">"),
            Self::LE => Some("<="),
            Self::GE => Some(">="),
            Self::EQ_EQ => Some("=="),
            Self::BANG_EQ => Some("!="),
            Self::EQ => Some("="),
            Self::PIPE => Some("|"),
            Self::BANG => Some("!"),
            Self::DOT => Some("."),
            Self::COLON => Some(":"),
            Self::COLON_COLON => Some("::"),
            Self::L_PAREN => Some("("),
            Self::R_PAREN => Some(")"),
            Self::L_BRACE => Some("{"),
            Self::R_BRACE => Some("}"),
            Self::L_BRACKET => Some("["),
            Self::R_BRACKET => Some("]"),
            Self::COMMA => Some(","),
            Self::SEMICOLON => Some(";"),
            _ => None,
        }
    }
}

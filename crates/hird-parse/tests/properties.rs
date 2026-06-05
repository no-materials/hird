// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

// Property tests over the v0.1 surface grammar.
//
// A small generator builds well-formed programs (`fn`/`type` declarations
// hosting `let`/`lambda`/`match`/`if`/records/tuples/lists/application/binops)
// as an abstract tree, then renders that tree in two operator spellings. The
// properties are:
//
// 1. Lossless round-trip — a well-formed program reparses to a CST whose text
//    equals the source and whose diagnostic list is empty.
// 2. Spelling equivalence — the ASCII and Unicode operator spellings of one
//    program parse to the same CST structure (same node and token kinds). This
//    is the lexer's canonicalisation (`->` and `\u{2192}` are one token kind)
//    observed through the parser; the CST stays byte-for-byte lossless.
// 3. No panic on garbage — arbitrary text and random token streams always parse
//    to a (lossless) tree without panicking or hanging.

use std::fmt::Write;

use hird_parse::SyntaxKind;
use proptest::prelude::*;

// ── identifier pools (case-correct so the lexer never flags them) ───
//
// The lexer enforces canonical naming: lowercase-initial identifiers must be
// `snake_case`, uppercase-initial ones `PascalCase`. Drawing names from fixed
// pools keeps every generated identifier valid and keyword-free.

/// `snake_case` value identifiers (functions, params, bindings, fields).
const SNAKE: &[&str] = &[
    "x", "y", "z", "foo", "bar", "baz", "n", "acc", "item", "val", "tmp",
];
/// `PascalCase` names (types and constructors).
const PASCAL: &[&str] = &[
    "Foo", "Bar", "Baz", "Int", "Bool", "Node", "Some", "None", "Ok", "Nil",
];
/// Single-letter type variables.
const TYVAR: &[&str] = &["a", "b", "c", "r", "s"];

// ── surface AST for generation ──────────────────────────────────────

#[derive(Debug, Clone)]
enum Ty {
    Name(&'static str),
    App(&'static str, Vec<Self>),
    Tuple(Vec<Self>),
    Unit,
}

#[derive(Debug, Clone)]
enum Pat {
    Wild,
    Bind(&'static str),
    LitInt(u32),
    Ctor(&'static str, Vec<Self>),
    Tuple(Vec<Self>),
}

#[derive(Debug, Clone, Copy)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

#[derive(Debug, Clone)]
enum Expr {
    Int(u32),
    Float(u16, u16),
    Str(String),
    Var(&'static str),
    Ctor(&'static str),
    Paren(Box<Self>),
    Tuple(Vec<Self>),
    List(Vec<Self>),
    Record(Vec<(&'static str, Self)>),
    Lambda(Vec<&'static str>, Box<Self>),
    Let(&'static str, Option<Ty>, Box<Self>, Box<Self>),
    If(Box<Self>, Box<Self>, Box<Self>),
    Match(Box<Self>, Vec<(Pat, Self)>),
    App(Box<Self>, Vec<Self>),
    Bin(BinOp, Box<Self>, Box<Self>),
}

#[derive(Debug, Clone)]
enum Decl {
    Fn {
        name: &'static str,
        params: Vec<(&'static str, Ty)>,
        ret: Option<Ty>,
        body: Expr,
    },
    Type {
        name: &'static str,
        params: Vec<&'static str>,
        ctors: Vec<(&'static str, Vec<Ty>)>,
    },
}

// ── rendering ───────────────────────────────────────────────────────

/// Which spelling to emit for the operators the lexer canonicalises.
#[derive(Debug, Clone, Copy)]
enum Spelling {
    Ascii,
    Unicode,
}

impl Spelling {
    fn arrow(self) -> &'static str {
        match self {
            Self::Ascii => "->",
            Self::Unicode => "\u{2192}",
        }
    }

    fn lambda(self) -> &'static str {
        match self {
            Self::Ascii => "\\",
            Self::Unicode => "\u{03bb}",
        }
    }

    fn and(self) -> &'static str {
        match self {
            Self::Ascii => "&&",
            Self::Unicode => "\u{2227}",
        }
    }

    fn or(self) -> &'static str {
        match self {
            Self::Ascii => "||",
            Self::Unicode => "\u{2228}",
        }
    }
}

impl BinOp {
    fn spelling(self, sp: Spelling) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::And => sp.and(),
            Self::Or => sp.or(),
        }
    }
}

fn render_ty(t: &Ty) -> String {
    match t {
        Ty::Name(n) => (*n).to_string(),
        Ty::App(n, args) => format!("{n}<{}>", join(args.iter().map(render_ty), ", ")),
        Ty::Tuple(tys) => format!("({})", join(tys.iter().map(render_ty), ", ")),
        Ty::Unit => "()".to_string(),
    }
}

fn render_pat(p: &Pat) -> String {
    match p {
        Pat::Wild => "_".to_string(),
        Pat::Bind(s) => (*s).to_string(),
        Pat::LitInt(n) => n.to_string(),
        Pat::Ctor(n, args) if args.is_empty() => (*n).to_string(),
        Pat::Ctor(n, args) => format!("{n}({})", join(args.iter().map(render_pat), ", ")),
        Pat::Tuple(ps) => format!("({})", join(ps.iter().map(render_pat), ", ")),
    }
}

fn render_expr(e: &Expr, sp: Spelling) -> String {
    match e {
        Expr::Int(n) => n.to_string(),
        Expr::Float(a, b) => format!("{a}.{b}"),
        Expr::Str(s) => format!("\"{s}\""),
        Expr::Var(s) | Expr::Ctor(s) => (*s).to_string(),
        Expr::Paren(e) => format!("({})", render_expr(e, sp)),
        Expr::Tuple(es) => format!("({})", join_exprs(es, sp, ", ")),
        Expr::List(es) => format!("[{}]", join_exprs(es, sp, ", ")),
        Expr::Record(fields) if fields.is_empty() => "{}".to_string(),
        Expr::Record(fields) => {
            let body = fields
                .iter()
                .map(|(n, v)| format!("{n}: {}", render_expr(v, sp)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {body} }}")
        }
        Expr::Lambda(ps, b) => format!(
            "{} {} {} {}",
            sp.lambda(),
            ps.join(" "),
            sp.arrow(),
            render_expr(b, sp)
        ),
        Expr::Let(x, Some(t), e, body) => format!(
            "let {x} : {} = {} in {}",
            render_ty(t),
            render_expr(e, sp),
            render_expr(body, sp)
        ),
        Expr::Let(x, None, e, body) => {
            format!(
                "let {x} = {} in {}",
                render_expr(e, sp),
                render_expr(body, sp)
            )
        }
        Expr::If(c, t, e) => format!(
            "if {} then {} else {}",
            render_expr(c, sp),
            render_expr(t, sp),
            render_expr(e, sp)
        ),
        Expr::Match(scrut, arms) => {
            let body = arms
                .iter()
                .map(|(p, b)| format!("{} {} {}", render_pat(p), sp.arrow(), render_expr(b, sp)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("match {} {{ {body} }}", render_expr(scrut, sp))
        }
        Expr::App(callee, args) => {
            format!("{} {}", render_expr(callee, sp), join_exprs(args, sp, " "))
        }
        // Every binary expression is fully parenthesised. This keeps the source
        // unambiguous and, crucially, makes a non-associative chain (`a == b ==
        // c`, a P0005) impossible: each comparison sits alone inside its parens.
        Expr::Bin(op, l, r) => format!(
            "({} {} {})",
            render_expr(l, sp),
            op.spelling(sp),
            render_expr(r, sp)
        ),
    }
}

fn render_decl(d: &Decl, sp: Spelling) -> String {
    match d {
        Decl::Fn {
            name,
            params,
            ret,
            body,
        } => {
            let ps = params
                .iter()
                .map(|(n, t)| format!("{n}: {}", render_ty(t)))
                .collect::<Vec<_>>()
                .join(", ");
            let r = match ret {
                Some(t) => format!(" {} {}", sp.arrow(), render_ty(t)),
                None => String::new(),
            };
            format!("fn {name}({ps}){r} = {}", render_expr(body, sp))
        }
        Decl::Type {
            name,
            params,
            ctors,
        } => {
            let ps = if params.is_empty() {
                String::new()
            } else {
                format!("<{}>", params.join(", "))
            };
            let cs = ctors
                .iter()
                .map(|(n, fields)| {
                    if fields.is_empty() {
                        (*n).to_string()
                    } else {
                        format!("{n}({})", join(fields.iter().map(render_ty), ", "))
                    }
                })
                .collect::<Vec<_>>()
                .join(" | ");
            format!("type {name}{ps} = {cs}")
        }
    }
}

fn render_program(decls: &[Decl], sp: Spelling) -> String {
    decls
        .iter()
        .map(|d| render_decl(d, sp))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn join_exprs(es: &[Expr], sp: Spelling, sep: &str) -> String {
    es.iter()
        .map(|e| render_expr(e, sp))
        .collect::<Vec<_>>()
        .join(sep)
}

fn join(parts: impl Iterator<Item = String>, sep: &str) -> String {
    parts.collect::<Vec<_>>().join(sep)
}

// ── CST structure projection (kinds only, no text) ──────────────────

/// Renders a parsed CST as an indented tree of node and token *kinds*, dropping
/// all source text. Two programs that differ only in operator spelling
/// (`->` vs `\u{2192}`) produce byte-identical output here.
fn kind_structure(source: &str) -> String {
    let parsed = hird_parse::parse(source, 0);
    let root = cstree::syntax::SyntaxNode::<SyntaxKind>::new_root(parsed.green().clone());
    let mut out = String::new();
    walk_kinds(&root, &mut out, 0);
    out
}

fn walk_kinds(node: &cstree::syntax::SyntaxNode<SyntaxKind>, out: &mut String, depth: usize) {
    let pad = "  ".repeat(depth);
    writeln!(out, "{pad}{:?}", node.kind()).unwrap();
    for child in node.children_with_tokens() {
        match child {
            cstree::util::NodeOrToken::Node(n) => walk_kinds(n, out, depth + 1),
            cstree::util::NodeOrToken::Token(t) => {
                let pad = "  ".repeat(depth + 1);
                writeln!(out, "{pad}{:?}", t.kind()).unwrap();
            }
        }
    }
}

// ── strategies ──────────────────────────────────────────────────────

fn snake() -> impl Strategy<Value = &'static str> + Clone {
    proptest::sample::select(SNAKE)
}

fn pascal() -> impl Strategy<Value = &'static str> + Clone {
    proptest::sample::select(PASCAL)
}

fn tyvar() -> impl Strategy<Value = &'static str> + Clone {
    proptest::sample::select(TYVAR)
}

fn binop() -> impl Strategy<Value = BinOp> + Clone {
    prop_oneof![
        Just(BinOp::Add),
        Just(BinOp::Sub),
        Just(BinOp::Mul),
        Just(BinOp::Div),
        Just(BinOp::Lt),
        Just(BinOp::Le),
        Just(BinOp::Gt),
        Just(BinOp::Ge),
        Just(BinOp::Eq),
        Just(BinOp::Ne),
        Just(BinOp::And),
        Just(BinOp::Or),
    ]
}

fn ty() -> BoxedStrategy<Ty> {
    let leaf = prop_oneof![
        pascal().prop_map(Ty::Name),
        tyvar().prop_map(Ty::Name),
        Just(Ty::Unit),
    ];
    leaf.prop_recursive(2, 12, 3, |inner| {
        prop_oneof![
            (pascal(), proptest::collection::vec(inner.clone(), 1..3))
                .prop_map(|(n, a)| Ty::App(n, a)),
            proptest::collection::vec(inner, 2..4).prop_map(Ty::Tuple),
        ]
    })
    .boxed()
}

fn pat() -> BoxedStrategy<Pat> {
    let leaf = prop_oneof![
        Just(Pat::Wild),
        snake().prop_map(Pat::Bind),
        any::<u32>().prop_map(Pat::LitInt),
        pascal().prop_map(|n| Pat::Ctor(n, Vec::new())),
    ];
    leaf.prop_recursive(2, 12, 3, |inner| {
        prop_oneof![
            (pascal(), proptest::collection::vec(inner.clone(), 1..3))
                .prop_map(|(n, a)| Pat::Ctor(n, a)),
            proptest::collection::vec(inner, 2..4).prop_map(Pat::Tuple),
        ]
    })
    .boxed()
}

fn expr() -> BoxedStrategy<Expr> {
    let leaf = prop_oneof![
        any::<u32>().prop_map(Expr::Int),
        (any::<u16>(), any::<u16>()).prop_map(|(a, b)| Expr::Float(a, b)),
        "[a-zA-Z0-9 ]{0,8}".prop_map(Expr::Str),
        snake().prop_map(Expr::Var),
        pascal().prop_map(Expr::Ctor),
    ];
    leaf.prop_recursive(4, 64, 6, |inner| {
        // An application argument must begin with an atom token (ident, literal,
        // `(`, or `[`) — a bare keyword form (`if`, `match`, `\u{03bb}`...) is
        // not an argument, so complex exprs reach argument position via `( … )`.
        let arg = prop_oneof![
            snake().prop_map(Expr::Var),
            pascal().prop_map(Expr::Ctor),
            any::<u32>().prop_map(Expr::Int),
            inner.clone().prop_map(|e| Expr::Paren(Box::new(e))),
            proptest::collection::vec(inner.clone(), 0..3).prop_map(Expr::List),
        ];
        let callee = prop_oneof![
            snake().prop_map(Expr::Var),
            pascal().prop_map(Expr::Ctor),
            inner.clone().prop_map(|e| Expr::Paren(Box::new(e))),
        ];
        prop_oneof![
            inner.clone().prop_map(|e| Expr::Paren(Box::new(e))),
            proptest::collection::vec(inner.clone(), 2..4).prop_map(Expr::Tuple),
            proptest::collection::vec(inner.clone(), 0..4).prop_map(Expr::List),
            proptest::collection::vec((snake(), inner.clone()), 0..3).prop_map(Expr::Record),
            (proptest::collection::vec(snake(), 1..3), inner.clone())
                .prop_map(|(ps, b)| Expr::Lambda(ps, Box::new(b))),
            (
                snake(),
                proptest::option::of(ty()),
                inner.clone(),
                inner.clone()
            )
                .prop_map(|(x, t, e, b)| Expr::Let(x, t, Box::new(e), Box::new(b))),
            (inner.clone(), inner.clone(), inner.clone()).prop_map(|(c, t, e)| Expr::If(
                Box::new(c),
                Box::new(t),
                Box::new(e)
            )),
            (
                inner.clone(),
                proptest::collection::vec((pat(), inner.clone()), 1..3)
            )
                .prop_map(|(s, arms)| Expr::Match(Box::new(s), arms)),
            (callee, proptest::collection::vec(arg, 1..3))
                .prop_map(|(c, a)| Expr::App(Box::new(c), a)),
            (binop(), inner.clone(), inner.clone()).prop_map(|(op, l, r)| Expr::Bin(
                op,
                Box::new(l),
                Box::new(r)
            )),
        ]
    })
    .boxed()
}

fn decl() -> BoxedStrategy<Decl> {
    let fn_decl = (
        snake(),
        proptest::collection::vec((snake(), ty()), 0..3),
        proptest::option::of(ty()),
        expr(),
    )
        .prop_map(|(name, params, ret, body)| Decl::Fn {
            name,
            params,
            ret,
            body,
        });
    let type_decl = (
        pascal(),
        proptest::collection::vec(tyvar(), 0..3),
        proptest::collection::vec((pascal(), proptest::collection::vec(ty(), 0..3)), 1..3),
    )
        .prop_map(|(name, params, ctors)| Decl::Type {
            name,
            params,
            ctors,
        });
    prop_oneof![fn_decl, type_decl].boxed()
}

fn program() -> impl Strategy<Value = Vec<Decl>> {
    proptest::collection::vec(decl(), 1..4)
}

/// A stream of random Hird lexemes (both operator spellings, delimiters,
/// keywords, names, literals) joined by spaces — structured garbage that drives
/// deeper parser paths than free-form text.
fn token_soup() -> impl Strategy<Value = String> {
    const VOCAB: &[&str] = &[
        "fn",
        "type",
        "let",
        "in",
        "if",
        "then",
        "else",
        "match",
        "handle",
        "actor",
        "supervisor",
        "effect",
        "tool",
        "extern",
        "use",
        "pub",
        "module",
        "(",
        ")",
        "{",
        "}",
        "[",
        "]",
        ",",
        ";",
        ":",
        "::",
        ".",
        "->",
        "\u{2192}",
        "=>",
        "\u{21d2}",
        "\\",
        "\u{03bb}",
        "&&",
        "\u{2227}",
        "||",
        "\u{2228}",
        "|",
        "!",
        "=",
        "==",
        "!=",
        "<",
        ">",
        "<=",
        ">=",
        "+",
        "-",
        "*",
        "/",
        "x",
        "Foo",
        "_",
        "42",
        "3.14",
        "\"s\"",
        "a",
    ];
    proptest::collection::vec(proptest::sample::select(VOCAB), 0..40)
        .prop_map(|toks| toks.join(" "))
}

// ── properties ──────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Well-formed programs reparse losslessly with no diagnostics, in either
    /// operator spelling.
    #[test]
    fn well_formed_programs_round_trip(prog in program()) {
        for sp in [Spelling::Ascii, Spelling::Unicode] {
            let src = render_program(&prog, sp);
            let parsed = hird_parse::parse(&src, 0);
            prop_assert!(
                parsed.syntax().text() == src.as_str(),
                "CST is not lossless for:\n{src}"
            );
            prop_assert!(
                parsed.is_ok(),
                "unexpected diagnostics for:\n{src}\n{:?}",
                parsed.diagnostics()
            );
        }
    }

    /// The ASCII and Unicode spellings of one program parse to the same CST
    /// structure: identical node and token kinds throughout.
    #[test]
    fn ascii_and_unicode_spellings_share_structure(prog in program()) {
        let ascii = render_program(&prog, Spelling::Ascii);
        let unicode = render_program(&prog, Spelling::Unicode);
        prop_assert_eq!(
            kind_structure(&ascii),
            kind_structure(&unicode),
            "token-kind structure differs:\nascii:\n{}\nunicode:\n{}",
            ascii,
            unicode
        );
    }

    /// Arbitrary text never panics and always yields a lossless tree.
    #[test]
    fn arbitrary_input_never_panics(s in any::<String>()) {
        let parsed = hird_parse::parse(&s, 0);
        prop_assert!(parsed.syntax().text() == s.as_str(), "CST is not lossless for {s:?}");
    }

    /// Random token streams never panic and always yield a lossless tree.
    #[test]
    fn token_soup_never_panics(s in token_soup()) {
        let parsed = hird_parse::parse(&s, 0);
        prop_assert!(parsed.syntax().text() == s.as_str(), "CST is not lossless for {s:?}");
    }
}

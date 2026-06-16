// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hirð type checking: Hindley-Milner inference with let-polymorphism over
//! the typed AST.
//!
//! The checker walks a parsed [`SourceFile`] and produces a [`CheckedFile`]:
//! a side-table of inferred types keyed by CST node identity, the resolved
//! top-level bindings, the ADT shape table, and accumulated diagnostics. The
//! CST remains the single source of truth — no parallel typed tree is built.
//!
//! Top-level functions are checked in dependency order (strongly connected
//! components of the reference graph), so forward references work and
//! declaration order is not semantic. Recursion inside a component is
//! monomorphic unless a full signature annotation is given. Errors are
//! isolated per declaration: a type error stops that declaration's body
//! check but the rest of the file is still checked.
//!
//! Effect annotations (`! { … }`) are parsed but ignored by this pass.
//!
//! # Quick start
//!
//! ```
//! use hird_ast::{AstNode, SourceFile};
//!
//! let parsed = hird_parse::parse("fn answer() = 42", 0);
//! let file = SourceFile::cast(parsed.syntax().clone()).unwrap();
//! let checked = hird_check::check(&file, 0);
//! assert!(checked.diagnostics.is_empty());
//! assert_eq!(
//!     checked.bindings["answer"].to_string(),
//!     "() \u{2192} Int",
//! );
//! ```

#![no_std]

extern crate alloc;

mod checker;
mod diag;
mod elaborate;
mod env;
mod infer;
mod registry;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use hird_ast::{Expr, SourceFile, SyntaxNode, SyntaxToken};
use hird_lex::Span;
use hird_parse::SyntaxKind;
use hird_types::{Name, Type};

pub use diag::{CheckCode, CheckDiagnostic, Severity};

/// Identity of a CST node or token: kind plus byte range.
///
/// The kind disambiguates the rare case of a node whose single child covers
/// its full extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeKey {
    /// The node or token kind, as its raw representation.
    kind: u32,
    /// Byte offset of the first byte (inclusive).
    start: u32,
    /// Byte offset past the last byte (exclusive).
    end: u32,
}

impl NodeKey {
    /// The key of a CST node.
    #[must_use]
    pub fn of_node(node: &SyntaxNode) -> Self {
        let range = node.text_range();
        Self {
            kind: node.kind() as u32,
            start: range.start().into(),
            end: range.end().into(),
        }
    }

    /// The key of a CST token.
    #[must_use]
    pub fn of_token(token: &SyntaxToken) -> Self {
        let range = token.text_range();
        Self {
            kind: token.kind() as u32,
            start: range.start().into(),
            end: range.end().into(),
        }
    }

    /// The key of an expression, whichever of node or token backs it.
    #[must_use]
    pub fn of_expr(expr: &Expr) -> Self {
        match expr {
            Expr::Literal(lit) => Self::of_token(lit.syntax()),
            Expr::Name(name) => Self::of_token(name.syntax()),
            other => Self::of_node(other.syntax().expect("non-atomic exprs are nodes")),
        }
    }
}

/// Result of checking one source file.
#[derive(Debug)]
pub struct CheckedFile {
    /// Inferred type of every visited expression, pattern, and declaration,
    /// keyed by CST identity and fully resolved.
    pub types: BTreeMap<NodeKey, Type>,
    /// Resolved top-level value bindings: functions, externs, and
    /// constructors. Schemes render with [`Type::normalized`].
    pub bindings: BTreeMap<String, Type>,
    /// Declared ADTs (including the built-in `Bool`) and their constructor
    /// names in declaration order.
    pub adts: BTreeMap<Name, Vec<Name>>,
    /// Errors and warnings, in source order.
    pub diagnostics: Vec<CheckDiagnostic>,
}

impl CheckedFile {
    /// The inferred type recorded for `key`, if any.
    #[must_use]
    pub fn type_at(&self, key: NodeKey) -> Option<&Type> {
        self.types.get(&key)
    }

    /// Whether any diagnostic is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

/// Type-checks `file`, attributing diagnostics to `source_id`.
///
/// The input is expected to be parse-error-free; declarations with missing
/// pieces (parser recovery artefacts) are skipped without further
/// diagnostics.
#[must_use]
pub fn check(file: &SourceFile, source_id: u32) -> CheckedFile {
    checker::Checker::new(source_id).run(file)
}

/// The span of a CST node.
pub(crate) fn node_span(node: &SyntaxNode, source_id: u32) -> Span {
    let range = node.text_range();
    Span::new(range.start().into(), range.end().into(), source_id)
}

/// The span of a CST token.
pub(crate) fn token_span(token: &SyntaxToken, source_id: u32) -> Span {
    let range = token.text_range();
    Span::new(range.start().into(), range.end().into(), source_id)
}

/// The span of an expression, whichever of node or token backs it.
pub(crate) fn expr_span(expr: &Expr, source_id: u32) -> Span {
    match expr {
        Expr::Literal(lit) => token_span(lit.syntax(), source_id),
        Expr::Name(name) => token_span(name.syntax(), source_id),
        other => node_span(
            other.syntax().expect("non-atomic exprs are nodes"),
            source_id,
        ),
    }
}

/// The span of a type expression, whichever of node or token backs it.
pub(crate) fn type_expr_span(ty: &hird_ast::TypeExpr, source_id: u32) -> Span {
    match ty {
        hird_ast::TypeExpr::Name(name) => token_span(name.syntax(), source_id),
        other => node_span(
            other.syntax().expect("non-atomic type exprs are nodes"),
            source_id,
        ),
    }
}

/// The span of the first `IDENT` token directly under `node` (a
/// declaration's or binder's name).
pub(crate) fn name_token_span(node: &SyntaxNode, source_id: u32) -> Span {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map_or_else(|| node_span(node, source_id), |t| token_span(t, source_id))
}

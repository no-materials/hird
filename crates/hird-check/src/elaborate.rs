// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Elaboration of surface type expressions into semantic [`Type`]s.
//!
//! Surface names classify by case (the lexer enforces canonical naming):
//! lowercase names are type variables, `PascalCase` names are type
//! constructors resolved through the registry. How an unknown type variable
//! resolves depends on the position:
//!
//! - **closed** (type declarations): only the declared parameters are in
//!   scope; anything else is an error.
//! - **fresh** (inferred signatures and `let` annotations): each distinct
//!   name maps to a fresh unification variable, implicitly quantified by the
//!   surrounding generalisation.
//! - **skolem** (fully annotated function bodies): each distinct name maps
//!   to a rigid constant, so the body cannot specialise the signature.
//!
//! Variable scopes are per annotation site; a flexible variable in an inner
//! annotation unifies with anything the context demands, so the loss of
//! lexical type-variable scoping is benign.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_ast::TypeExpr;
use hird_types::Type;

use crate::checker::{Aborted, Checked, Checker};
use crate::diag::CheckCode;
use crate::{token_span, type_expr_span};

/// How an out-of-scope type-variable name resolves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VarMode {
    /// Error: the surrounding declaration binds all variables explicitly.
    Closed,
    /// A fresh unification variable.
    Fresh,
    /// A rigid skolem constant.
    Skolem,
}

impl Checker {
    /// Elaborates with declared-parameters-only scoping (type declarations).
    pub(crate) fn elaborate_closed(
        &mut self,
        ty: &TypeExpr,
        scope: &mut BTreeMap<String, Type>,
    ) -> Checked<Type> {
        self.elaborate(ty, scope, VarMode::Closed)
    }

    /// Elaborates with implicit fresh variables (inferred positions).
    pub(crate) fn elaborate_fresh(
        &mut self,
        ty: &TypeExpr,
        scope: &mut BTreeMap<String, Type>,
    ) -> Checked<Type> {
        self.elaborate(ty, scope, VarMode::Fresh)
    }

    /// Elaborates with rigid skolem variables (annotated function bodies).
    pub(crate) fn elaborate_skolem(
        &mut self,
        ty: &TypeExpr,
        scope: &mut BTreeMap<String, Type>,
    ) -> Checked<Type> {
        self.elaborate(ty, scope, VarMode::Skolem)
    }

    /// Elaborates `ty`, resolving variable names through `scope` and
    /// constructor names through the registry.
    fn elaborate(
        &mut self,
        ty: &TypeExpr,
        scope: &mut BTreeMap<String, Type>,
        mode: VarMode,
    ) -> Checked<Type> {
        match ty {
            TypeExpr::Name(name) => {
                let text = name.text();
                let span = token_span(name.syntax(), self.source_id);
                if is_var_name(text) {
                    if let Some(bound) = scope.get(text) {
                        return Ok(bound.clone());
                    }
                    match mode {
                        VarMode::Closed => Err(self.error(
                            CheckCode::C0012,
                            span,
                            format!("type parameter `{text}` is not declared here"),
                        )),
                        VarMode::Fresh => {
                            let fresh = self.subst.fresh_type();
                            scope.insert(String::from(text), fresh.clone());
                            Ok(fresh)
                        }
                        VarMode::Skolem => {
                            // Lowercase constructor names are unutterable in
                            // source, so a skolem can never collide with a
                            // declared type, and it renders as the variable
                            // it stands for.
                            let skolem = Type::con(text, Vec::new());
                            scope.insert(String::from(text), skolem.clone());
                            Ok(skolem)
                        }
                    }
                } else {
                    self.named_type(text, Vec::new(), span)
                }
            }
            TypeExpr::App(app) => {
                let span = type_expr_span(ty, self.source_id);
                let Some(name) = app.name() else {
                    return Err(Aborted);
                };
                if is_var_name(name) {
                    return Err(self.error(
                        CheckCode::C0004,
                        span,
                        format!("type variable `{name}` cannot take type arguments"),
                    ));
                }
                let mut args = Vec::new();
                for arg in app.args() {
                    args.push(self.elaborate(&arg, scope, mode)?);
                }
                self.named_type(name, args, span)
            }
            TypeExpr::Fn(func) => {
                let mut params = Vec::new();
                for param in func.params() {
                    params.push(self.elaborate(&param, scope, mode)?);
                }
                let Some(ret) = func.return_type() else {
                    return Err(Aborted);
                };
                let ret = self.elaborate(&ret, scope, mode)?;
                Ok(Type::func(params, ret))
            }
            TypeExpr::Tuple(tuple) => {
                let mut elems = Vec::new();
                for elem in tuple.elements() {
                    elems.push(self.elaborate(&elem, scope, mode)?);
                }
                Ok(Type::tuple(elems))
            }
            TypeExpr::Paren(paren) => {
                let Some(inner) = paren.inner() else {
                    return Err(Aborted);
                };
                self.elaborate(&inner, scope, mode)
            }
        }
    }

    /// Resolves a constructor application against the registry, checking
    /// arity.
    fn named_type(&mut self, name: &str, args: Vec<Type>, span: hird_lex::Span) -> Checked<Type> {
        match self.registry.type_arity(name) {
            None => Err(self.error(CheckCode::C0004, span, format!("unknown type `{name}`"))),
            Some(arity) if arity != args.len() => Err(self.error(
                CheckCode::C0005,
                span,
                format!(
                    "`{name}` expects {arity} type argument(s), but {} were given",
                    args.len()
                ),
            )),
            Some(_) => Ok(Type::con(name, args)),
        }
    }
}

/// Whether a surface type name is a type variable (lowercase) rather than a
/// constructor (`PascalCase`).
fn is_var_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_lowercase)
}

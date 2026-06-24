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

use hird_ast::{EffectAnn, TypeExpr};
use hird_types::{Effect, EffectRow, RowVar, Type};

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

/// Per-annotation-site variable scopes, threaded through elaboration so a name
/// used in several positions of one signature resolves to the same variable.
/// Type variables and row variables are separate namespaces, distinguished by
/// position: a name inside `! { … }` is a row variable, elsewhere a type one.
#[derive(Default)]
pub(crate) struct Scope {
    /// Type-variable names to their elaborated types.
    types: BTreeMap<String, Type>,
    /// Row-variable names to their allocated row variables.
    rows: BTreeMap<String, RowVar>,
}

impl Scope {
    /// A fresh, empty scope.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Pre-binds a type-variable name (a type declaration's declared
    /// parameters, fixed before elaboration begins).
    pub(crate) fn insert_type(&mut self, name: String, ty: Type) {
        self.types.insert(name, ty);
    }
}

/// One elaborated effect-row entry.
enum RowEntry {
    /// A concrete effect to add to the row.
    Effect(Effect),
    /// The row's tail variable, named by a bare lowercase entry.
    Tail(RowVar),
}

impl Checker {
    /// Elaborates with declared-parameters-only scoping (type declarations).
    pub(crate) fn elaborate_closed(&mut self, ty: &TypeExpr, scope: &mut Scope) -> Checked<Type> {
        self.elaborate(ty, scope, VarMode::Closed)
    }

    /// Elaborates with implicit fresh variables (inferred positions).
    pub(crate) fn elaborate_fresh(&mut self, ty: &TypeExpr, scope: &mut Scope) -> Checked<Type> {
        self.elaborate(ty, scope, VarMode::Fresh)
    }

    /// Elaborates with rigid skolem variables (annotated function bodies).
    pub(crate) fn elaborate_skolem(&mut self, ty: &TypeExpr, scope: &mut Scope) -> Checked<Type> {
        self.elaborate(ty, scope, VarMode::Skolem)
    }

    /// Elaborates a function's effect-row annotation with implicit fresh row
    /// variables (a signature position, alongside [`Checker::elaborate_fresh`]).
    pub(crate) fn elaborate_row_fresh(
        &mut self,
        ann: &EffectAnn,
        scope: &mut Scope,
    ) -> Checked<EffectRow> {
        self.elaborate_effect_row(ann, scope, VarMode::Fresh)
    }

    /// Elaborates `ty`, resolving variable names through `scope` and
    /// constructor names through the registry.
    fn elaborate(&mut self, ty: &TypeExpr, scope: &mut Scope, mode: VarMode) -> Checked<Type> {
        match ty {
            TypeExpr::Name(name) => {
                let text = name.text();
                let span = token_span(name.syntax(), self.source_id);
                if is_var_name(text) {
                    if let Some(bound) = scope.types.get(text) {
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
                            scope.types.insert(String::from(text), fresh.clone());
                            Ok(fresh)
                        }
                        VarMode::Skolem => {
                            // Lowercase constructor names are unutterable in
                            // source, so a skolem can never collide with a
                            // declared type, and it renders as the variable
                            // it stands for.
                            let skolem = Type::con(text, Vec::new());
                            scope.types.insert(String::from(text), skolem.clone());
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
                match func.effect_ann() {
                    Some(ann) => {
                        let row = self.elaborate_effect_row(&ann, scope, mode)?;
                        Ok(Type::func_eff(params, ret, row))
                    }
                    None => Ok(Type::func(params, ret)),
                }
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

    /// Elaborates an effect-row annotation (`! { E1, E2 }`) into an
    /// [`EffectRow`]. A lowercase entry is the row's tail variable (at most one
    /// per row); a `PascalCase` entry — bare or applied — is an effect, checked
    /// for declaration and arity against the registry.
    fn elaborate_effect_row(
        &mut self,
        ann: &EffectAnn,
        scope: &mut Scope,
        mode: VarMode,
    ) -> Checked<EffectRow> {
        let mut row = EffectRow::empty();
        for entry in ann.effects() {
            match self.elaborate_effect(&entry, scope, mode)? {
                RowEntry::Effect(effect) => row.insert(effect),
                RowEntry::Tail(var) => {
                    // A lowercase entry is the row tail. Only one is allowed.
                    if row.tail().is_some_and(|existing| existing != var) {
                        return Err(self.error(
                            CheckCode::C0029,
                            type_expr_span(&entry, self.source_id),
                            String::from("an effect row may name at most one row variable"),
                        ));
                    }
                    row = row.with_tail(Some(var));
                }
            }
        }
        Ok(row)
    }

    /// Elaborates one effect-row entry: a concrete effect, or the row's tail
    /// variable for a bare lowercase name. Unknown effects and arity mismatches
    /// are errors.
    fn elaborate_effect(
        &mut self,
        entry: &TypeExpr,
        scope: &mut Scope,
        mode: VarMode,
    ) -> Checked<RowEntry> {
        match entry {
            TypeExpr::Name(name) if is_var_name(name.text()) => {
                let span = type_expr_span(entry, self.source_id);
                self.row_var(name.text(), span, scope, mode)
                    .map(RowEntry::Tail)
            }
            TypeExpr::Name(name) => {
                let span = token_span(name.syntax(), self.source_id);
                self.named_effect(name.text(), Vec::new(), span)
                    .map(RowEntry::Effect)
            }
            TypeExpr::App(app) => {
                let span = type_expr_span(entry, self.source_id);
                let Some(name) = app.name() else {
                    return Err(Aborted);
                };
                if is_var_name(name) {
                    return Err(self.error(
                        CheckCode::C0027,
                        span,
                        format!("`{name}` is a row variable and cannot take arguments"),
                    ));
                }
                let mut args = Vec::new();
                for arg in app.args() {
                    args.push(self.elaborate(&arg, scope, mode)?);
                }
                self.named_effect(name, args, span).map(RowEntry::Effect)
            }
            // Functions, tuples, and parentheses are not effects.
            other => {
                let span = type_expr_span(other, self.source_id);
                Err(self.error(
                    CheckCode::C0027,
                    span,
                    String::from("expected an effect, e.g. `Log` or `Tool<ReadRepo>`"),
                ))
            }
        }
    }

    /// Builds an effect after checking it is declared and applied to the right
    /// number of type arguments.
    fn named_effect(
        &mut self,
        name: &str,
        args: Vec<Type>,
        span: hird_lex::Span,
    ) -> Checked<Effect> {
        match self.registry.effect_arity(name) {
            None => Err(self.error(CheckCode::C0027, span, format!("unknown effect `{name}`"))),
            Some(arity) if arity != args.len() => Err(self.error(
                CheckCode::C0028,
                span,
                format!(
                    "effect `{name}` expects {arity} type argument(s), but {} were given",
                    args.len()
                ),
            )),
            Some(_) if args.is_empty() => Ok(Effect::named(name)),
            Some(_) => Ok(Effect::parametric(name, args)),
        }
    }

    /// Resolves a row-variable name through the scope, allocating per `mode`:
    /// an error in a closed position, otherwise a fresh row variable bound for
    /// the rest of the annotation site.
    fn row_var(
        &mut self,
        text: &str,
        span: hird_lex::Span,
        scope: &mut Scope,
        mode: VarMode,
    ) -> Checked<RowVar> {
        if let Some(existing) = scope.rows.get(text) {
            return Ok(*existing);
        }
        if mode == VarMode::Closed {
            return Err(self.error(
                CheckCode::C0012,
                span,
                format!("row variable `{text}` is not declared here"),
            ));
        }
        // Body-vs-annotation effect checking is a later pass, so a fresh row
        // variable serves both inferred and annotated positions here.
        let var = self.subst.fresh_row();
        scope.rows.insert(String::from(text), var);
        Ok(var)
    }
}

/// Whether a surface type name is a type variable (lowercase) rather than a
/// constructor (`PascalCase`).
fn is_var_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_lowercase)
}

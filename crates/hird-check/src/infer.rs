// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Expression inference and pattern checking.
//!
//! Application follows the syntactic argument shape: a tuple-literal
//! argument is an n-ary argument list (`f(a, b)` is a 2-ary call, `f()` a
//! 0-ary one), anything else is a single argument, and a tuple *value* is
//! passed as `f((a, b))`. Operators are monomorphic (`Int` arithmetic and
//! ordering, polymorphic equality, `Bool` connectives). `handle` blocks
//! type as their body; effect handling is a later phase.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::mem;

use hird_ast::{
    AppExpr, AstNode, BinOpExpr, Expr, FieldExpr, IfExpr, LambdaExpr, LetExpr, MatchExpr, Pattern,
    RecordLit,
};
use hird_lex::Span;
use hird_parse::SyntaxKind;
use hird_types::{EffectRow, Label, Type};

use crate::checker::{Aborted, Checked, Checker};
use crate::diag::CheckCode;
use crate::elaborate::Scope;
use crate::registry::CtorInfo;
use crate::{ModuleName, NodeKey, expr_span, name_token_span, node_span, token_span};

impl Checker {
    /// Infers the type of `expr`, recording it in the node table.
    pub(crate) fn infer_expr(&mut self, expr: &Expr) -> Checked<Type> {
        let ty = self.infer_expr_inner(expr)?;
        self.types.push((NodeKey::of_expr(expr), ty.clone()));
        Ok(ty)
    }

    /// Dispatches over the expression forms.
    fn infer_expr_inner(&mut self, expr: &Expr) -> Checked<Type> {
        match expr {
            Expr::Literal(lit) => match lit.kind() {
                SyntaxKind::INT => Ok(Type::int()),
                SyntaxKind::FLOAT => Ok(Type::float()),
                SyntaxKind::STRING => Ok(Type::string()),
                _ => Err(Aborted),
            },
            Expr::Name(name) => {
                let text = name.text();
                let Some(scheme) = self.env.lookup(text).cloned() else {
                    let span = token_span(name.syntax(), self.source_id);
                    // A foreign opaque constructor is in the registry but never
                    // bound as a value, so construction outside its module lands
                    // here; name the type rather than report an unbound name.
                    if let Some(aborted) = self.opaque_construct_error(text, span) {
                        return Err(aborted);
                    }
                    return Err(self.error(
                        CheckCode::C0003,
                        span,
                        format!("unbound name `{text}`"),
                    ));
                };
                Ok(self.subst.instantiate(&scheme))
            }
            Expr::Let(le) => self.infer_let(le),
            Expr::Lambda(lambda) => self.infer_lambda(lambda),
            Expr::If(ife) => self.infer_if(ife),
            Expr::Match(me) => self.infer_match(me),
            Expr::Handle(handle) => {
                // DI-style handlers replace effect implementations without
                // changing the value type; arms are a later phase's concern.
                let Some(body) = handle.body() else {
                    return Err(Aborted);
                };
                self.infer_expr(&body)
            }
            Expr::BinOp(op) => self.infer_binop(op),
            Expr::App(app) => self.infer_app(app),
            Expr::Field(field) => self.infer_field(field),
            Expr::Record(record) => self.infer_record(record),
            Expr::Tuple(tuple) => {
                let mut elems = Vec::new();
                for elem in tuple.elements() {
                    elems.push(self.infer_expr(&elem)?);
                }
                Ok(Type::tuple(elems))
            }
            Expr::List(list) => {
                let elem_ty = self.subst.fresh_type();
                for elem in list.elements() {
                    let span = expr_span(&elem, self.source_id);
                    let ty = self.infer_expr(&elem)?;
                    self.unify_at(&elem_ty, &ty, span)?;
                }
                Ok(Type::list(elem_ty))
            }
            Expr::Paren(paren) => {
                let Some(inner) = paren.inner() else {
                    return Err(Aborted);
                };
                self.infer_expr(&inner)
            }
        }
    }

    /// `let name [: T] = value in body` — generalise the value, bind the
    /// scheme, infer the body.
    fn infer_let(&mut self, le: &LetExpr) -> Checked<Type> {
        let Some(name) = le.name() else {
            return Err(Aborted);
        };
        let name = String::from(name);
        let Some(value) = le.value() else {
            return Err(Aborted);
        };

        self.subst.enter_level();
        let mut value_ty = self.infer_expr(&value);
        if let Ok(ty) = &value_ty
            && let Some(annotation) = le.annotation()
        {
            let mut scope = Scope::new();
            let span = expr_span(&value, self.source_id);
            value_ty = match self.elaborate_fresh(&annotation, &mut scope) {
                Ok(ann_ty) => self.unify_at(&ann_ty, ty, span).map(|()| ann_ty),
                Err(aborted) => Err(aborted),
            };
        }
        self.subst.exit_level();
        let value_ty = value_ty?;
        let scheme = self.subst.generalize(&value_ty);

        self.env.push_scope();
        let span = name_token_span(le.syntax(), self.source_id);
        self.bind_value(&name, scheme, span);
        let body_ty = match le.body() {
            Some(body) => self.infer_expr(&body),
            None => Err(Aborted),
        };
        self.env.pop_scope();
        body_ty
    }

    /// `λx y → body` — monomorphic parameters, n-ary function type.
    fn infer_lambda(&mut self, lambda: &LambdaExpr) -> Checked<Type> {
        let mut params: Vec<(String, Span, NodeKey)> = Vec::new();
        for element in lambda.syntax().children_with_tokens() {
            let Some(token) = element.into_token() else {
                continue;
            };
            if token.kind() == SyntaxKind::ARROW {
                break;
            }
            if token.kind() == SyntaxKind::IDENT {
                params.push((
                    String::from(token.text()),
                    token_span(token, self.source_id),
                    NodeKey::of_token(token),
                ));
            }
        }

        let mut param_tys = Vec::new();
        self.env.push_scope();
        for (name, span, key) in &params {
            let ty = self.subst.fresh_type();
            self.types.push((*key, ty.clone()));
            self.bind_value(name, ty.clone(), *span);
            param_tys.push(ty);
        }
        // A lambda is pure as an expression: its body's effects belong to its
        // function type, not the enclosing row. Infer the body into a fresh
        // accumulator, then restore the enclosing one — discarding the lambda's
        // provenance, which the enclosing function never consults.
        let saved_row = mem::take(&mut self.current_row);
        let saved_prov = mem::take(&mut self.current_prov);
        let body_res = match lambda.body() {
            Some(body) => self.infer_expr(&body),
            None => Err(Aborted),
        };
        let body_row = mem::replace(&mut self.current_row, saved_row);
        self.current_prov = saved_prov;
        self.env.pop_scope();
        Ok(Type::func_eff(param_tys, body_res?, body_row))
    }

    /// `if c then a else b` — `Bool` condition, unified branches.
    fn infer_if(&mut self, ife: &IfExpr) -> Checked<Type> {
        let Some(cond) = ife.condition() else {
            return Err(Aborted);
        };
        let cond_span = expr_span(&cond, self.source_id);
        let cond_ty = self.infer_expr(&cond)?;
        self.unify_at(&Type::bool(), &cond_ty, cond_span)?;

        let Some(then_branch) = ife.then_branch() else {
            return Err(Aborted);
        };
        let then_ty = self.infer_expr(&then_branch)?;
        let Some(else_branch) = ife.else_branch() else {
            return Err(Aborted);
        };
        let else_span = expr_span(&else_branch, self.source_id);
        let else_ty = self.infer_expr(&else_branch)?;
        self.unify_at(&then_ty, &else_ty, else_span)?;
        Ok(then_ty)
    }

    /// `match scrutinee { arms }` — patterns check against the scrutinee,
    /// arm bodies unify together. Exhaustiveness is a separate pass.
    fn infer_match(&mut self, me: &MatchExpr) -> Checked<Type> {
        let Some(scrutinee) = me.scrutinee() else {
            return Err(Aborted);
        };
        let scrutinee_ty = self.infer_expr(&scrutinee)?;
        let mut result: Option<Type> = None;
        for arm in me.arms() {
            let Some(pattern) = arm.pattern() else {
                return Err(Aborted);
            };
            self.env.push_scope();
            let pattern_res = self.check_pattern(&pattern, &scrutinee_ty);
            let body_res = match (&pattern_res, arm.body()) {
                (Ok(()), Some(body)) => self
                    .infer_expr(&body)
                    .map(|ty| (ty, expr_span(&body, self.source_id))),
                _ => Err(Aborted),
            };
            self.env.pop_scope();
            pattern_res?;
            let (body_ty, body_span) = body_res?;
            match &result {
                None => result = Some(body_ty),
                Some(first) => self.unify_at(first, &body_ty, body_span)?,
            }
        }
        // Every arm checked cleanly: now decide coverage over the resolved
        // scrutinee type, which the constructor patterns have pinned down.
        let scrutinee_ty = self.subst.resolve(&scrutinee_ty);
        self.check_match(me, &scrutinee_ty);
        Ok(result.unwrap_or_else(|| self.subst.fresh_type()))
    }

    /// Checks `pattern` against the `expected` scrutinee type, binding
    /// pattern variables into the current scope.
    pub(crate) fn check_pattern(&mut self, pattern: &Pattern, expected: &Type) -> Checked<()> {
        self.types
            .push((NodeKey::of_node(pattern.syntax()), expected.clone()));
        let span = node_span(pattern.syntax(), self.source_id);
        match pattern {
            Pattern::Wildcard(_) => Ok(()),
            Pattern::Bind(bind) => {
                let Some(name) = bind.name() else {
                    return Err(Aborted);
                };
                self.bind_value(name, expected.clone(), span);
                Ok(())
            }
            Pattern::Literal(lit) => {
                let Some(literal) = lit.literal() else {
                    return Err(Aborted);
                };
                let ty = match literal.kind() {
                    SyntaxKind::INT => Type::int(),
                    SyntaxKind::FLOAT => Type::float(),
                    SyntaxKind::STRING => Type::string(),
                    _ => return Err(Aborted),
                };
                self.unify_at(expected, &ty, span)
            }
            Pattern::Tuple(tuple) => {
                let elements: Vec<Pattern> = tuple.elements().collect();
                let elem_tys: Vec<Type> =
                    elements.iter().map(|_| self.subst.fresh_type()).collect();
                self.unify_at(expected, &Type::tuple(elem_tys.clone()), span)?;
                for (element, ty) in elements.iter().zip(&elem_tys) {
                    self.check_pattern(element, ty)?;
                }
                Ok(())
            }
            Pattern::Constructor(ctor) => {
                let Some(name) = ctor.name() else {
                    return Err(Aborted);
                };
                // Snapshot what the gate needs, ending the registry borrow
                // before any diagnostic is pushed.
                let lookup = self.registry.ctor(name).map(|info| {
                    (
                        info.scheme.clone(),
                        opaque_violation(info, self.current_module.as_ref(), "destructure"),
                    )
                });
                let Some((scheme, violation)) = lookup else {
                    return Err(self.error(
                        CheckCode::C0007,
                        span,
                        format!("unknown constructor `{name}`"),
                    ));
                };
                if let Some(message) = violation {
                    return Err(self.error(CheckCode::C0021, span, message));
                }
                let instance = self.subst.instantiate(&scheme);
                let (fields, result_ty) = match instance {
                    Type::TyFn(params, ret, _) => (params, *ret),
                    other => (Vec::new(), other),
                };
                let sub_patterns: Vec<Pattern> = ctor.fields().collect();
                if sub_patterns.len() != fields.len() {
                    return Err(self.error(
                        CheckCode::C0008,
                        span,
                        format!(
                            "`{name}` has {} field(s), but the pattern names {}",
                            fields.len(),
                            sub_patterns.len()
                        ),
                    ));
                }
                self.unify_at(expected, &result_ty, span)?;
                for (sub, field_ty) in sub_patterns.iter().zip(&fields) {
                    self.check_pattern(sub, field_ty)?;
                }
                Ok(())
            }
        }
    }

    /// Binary operators per the v0.1 table: `Int` arithmetic and ordering,
    /// polymorphic equality, `Bool` connectives. No overloading.
    fn infer_binop(&mut self, op: &BinOpExpr) -> Checked<Type> {
        let Some(kind) = binop_kind(op) else {
            return Err(Aborted);
        };
        let Some(lhs) = op.lhs() else {
            return Err(Aborted);
        };
        let Some(rhs) = op.rhs() else {
            return Err(Aborted);
        };
        let lhs_span = expr_span(&lhs, self.source_id);
        let rhs_span = expr_span(&rhs, self.source_id);
        let lhs_ty = self.infer_expr(&lhs)?;
        let rhs_ty = self.infer_expr(&rhs)?;
        match kind {
            SyntaxKind::PLUS | SyntaxKind::MINUS | SyntaxKind::STAR | SyntaxKind::SLASH => {
                self.unify_at(&Type::int(), &lhs_ty, lhs_span)?;
                self.unify_at(&Type::int(), &rhs_ty, rhs_span)?;
                Ok(Type::int())
            }
            SyntaxKind::LT | SyntaxKind::LE | SyntaxKind::GT | SyntaxKind::GE => {
                self.unify_at(&Type::int(), &lhs_ty, lhs_span)?;
                self.unify_at(&Type::int(), &rhs_ty, rhs_span)?;
                Ok(Type::bool())
            }
            SyntaxKind::EQ_EQ | SyntaxKind::BANG_EQ => {
                self.unify_at(&lhs_ty, &rhs_ty, rhs_span)?;
                Ok(Type::bool())
            }
            SyntaxKind::ANDAND | SyntaxKind::OROR => {
                self.unify_at(&Type::bool(), &lhs_ty, lhs_span)?;
                self.unify_at(&Type::bool(), &rhs_ty, rhs_span)?;
                Ok(Type::bool())
            }
            _ => Err(Aborted),
        }
    }

    /// Application with syntactic argument shape (see module docs).
    fn infer_app(&mut self, app: &AppExpr) -> Checked<Type> {
        let Some(callee) = app.function() else {
            return Err(Aborted);
        };
        let callee_ty = self.infer_expr(&callee)?;
        // A tuple-literal argument is an argument list, not a tuple value;
        // it gets no entry in the node table because it is not an expression
        // in this position.
        let args: Vec<Expr> = match app.argument() {
            Some(Expr::Tuple(tuple)) => tuple.elements().collect(),
            Some(other) => Vec::from([other]),
            None => Vec::new(),
        };
        let mut arg_tys = Vec::new();
        for arg in &args {
            arg_tys.push(self.infer_expr(arg)?);
        }
        let app_span = node_span(app.syntax(), self.source_id);
        match self.subst.resolve(&callee_ty) {
            Type::TyFn(params, ret, row) => {
                if params.len() != arg_tys.len() {
                    return Err(self.error(
                        CheckCode::C0006,
                        app_span,
                        format!(
                            "this call supplies {} argument(s), but the function takes {}",
                            arg_tys.len(),
                            params.len()
                        ),
                    ));
                }
                for ((param, arg_ty), arg) in params.iter().zip(&arg_tys).zip(&args) {
                    let span = expr_span(arg, self.source_id);
                    self.unify_at(param, arg_ty, span)?;
                }
                // The callee's effects become the caller's, recorded at this call.
                self.add_effects(&row, app_span);
                Ok(*ret)
            }
            _ => {
                let ret = self.subst.fresh_type();
                // A not-yet-resolved callee gets a fresh effect-row variable, so
                // applying it stays effect-polymorphic (the row is generalised
                // for an interior function, or solved against the declared row
                // of a top-level one) rather than forced pure.
                let row = EffectRow::of_var(self.subst.fresh_row());
                let fn_ty = Type::func_eff(arg_tys, ret.clone(), row.clone());
                self.unify_at(&fn_ty, &callee_ty, app_span)?;
                self.add_effects(&row, app_span);
                Ok(ret)
            }
        }
    }

    /// Field access requires an already-determined record type; row
    /// polymorphism is a later phase.
    ///
    /// A bare name receiver that resolves in the module namespace makes this a
    /// qualified name (`Mod.member`) rather than field access; the `PascalCase`
    /// casing of module qualifiers keeps the two from ever overlapping with a
    /// value's field (`point.x`).
    fn infer_field(&mut self, field: &FieldExpr) -> Checked<Type> {
        let Some(receiver) = field.receiver() else {
            return Err(Aborted);
        };
        let Some(name) = field.field() else {
            return Err(Aborted);
        };
        let span = node_span(field.syntax(), self.source_id);
        if let Expr::Name(recv) = &receiver
            && let Some(member) = self
                .modules
                .get(recv.text())
                .map(|vals| vals.get(name).cloned())
        {
            // The receiver is a module qualifier: resolve against its exports
            // without ever typing the receiver as a value.
            return match member {
                Some(scheme) => Ok(self.subst.instantiate(&scheme)),
                None => Err(self.error(
                    CheckCode::C0024,
                    span,
                    format!("module `{}` has no exported value `{name}`", recv.text()),
                )),
            };
        }
        let receiver_ty = self.infer_expr(&receiver)?;
        match self.subst.resolve(&receiver_ty) {
            Type::TyRecord(fields) => match fields.get(&Label::new(name)) {
                Some(ty) => Ok(ty.clone()),
                None => {
                    let record = Type::TyRecord(fields.clone());
                    Err(self.error(
                        CheckCode::C0010,
                        span,
                        format!("record `{record}` has no field `{name}`"),
                    ))
                }
            },
            Type::TyVar(_) => Err(self.error(
                CheckCode::C0009,
                span,
                String::from(
                    "cannot determine the record type of this expression; add a type annotation",
                ),
            )),
            other => Err(self.error(
                CheckCode::C0009,
                span,
                format!("cannot access field `{name}` on non-record type `{other}`"),
            )),
        }
    }

    /// Reports constructing an opaque type outside its declaring module
    /// (C0022), if `name` is such a foreign opaque constructor. Returns `None`
    /// when `name` is not a constructor, or is one this module may construct.
    fn opaque_construct_error(&mut self, name: &str, span: Span) -> Option<Aborted> {
        let message = opaque_violation(
            self.registry.ctor(name)?,
            self.current_module.as_ref(),
            "construct",
        )?;
        Some(self.error(CheckCode::C0022, span, message))
    }

    /// `{ field: value, … }` — later duplicates override earlier ones.
    fn infer_record(&mut self, record: &RecordLit) -> Checked<Type> {
        let mut fields = Vec::new();
        for field in record.fields() {
            let Some(name) = field.name() else {
                return Err(Aborted);
            };
            let Some(value) = field.value() else {
                return Err(Aborted);
            };
            let ty = self.infer_expr(&value)?;
            fields.push((Label::new(name), ty));
        }
        Ok(Type::record(fields))
    }
}

/// The operator token kind of a binary expression.
fn binop_kind(op: &BinOpExpr) -> Option<SyntaxKind> {
    op.syntax()
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .map(|token| token.kind())
        .find(|kind| {
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
        })
}

/// The "outside its module" diagnostic for touching a foreign opaque
/// constructor `info`; `verb` is the action (`construct`/`destructure`).
/// `None` when `info` is transparent or owned by `current`. A module-less
/// owner (single-file checking) renders as the empty module name.
fn opaque_violation(info: &CtorInfo, current: Option<&ModuleName>, verb: &str) -> Option<String> {
    if !info.opaque || info.module.as_ref() == current {
        return None;
    }
    let module = info
        .module
        .as_ref()
        .map_or_else(String::new, |m| m.to_string());
    Some(format!(
        "cannot {verb} opaque type `{}` outside module `{module}`",
        info.owner
    ))
}

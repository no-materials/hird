// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Extern checking: Erlang-bound functions usable only as tool handlers.
//!
//! An `extern module "path.erl"` declares a hand-written Erlang source; an
//! `extern fn` binds one of its functions (`= "module":"function"`). An
//! extern's name is not a value: it may appear only as the whole handler of
//! a `Tool<…>` arm, so every foreign call goes through the tool dispatcher
//! and is recorded and replayable ([`CheckCode::C0066`]). Its row holds at
//! most the one `Exn<…>` the handled tool declares.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_ast::{AstNode, Expr, ExternDecl, ExternModuleDecl, SyntaxNode, SyntaxToken};
use hird_types::{EffectRow, Type};

use crate::checker::{Aborted, Checked, Checker};
use crate::diag::CheckCode;
use crate::{NodeKey, expr_span, name_token_span, node_span, token_span};

/// The module-name prefix generated and runtime modules use.
const RESERVED_PREFIX: &str = "hird_";

impl Checker {
    /// Records each `extern module` declaration's Erlang module name,
    /// reporting a malformed path or a module declared twice (C0065).
    pub(crate) fn register_extern_modules(&mut self, decls: &[ExternModuleDecl]) {
        for decl in decls {
            let Some(token) = decl.path_token() else {
                continue;
            };
            let span = token_span(token, self.source_id);
            let Some(module) = source_module(token) else {
                let _ = self.error(
                    CheckCode::C0065,
                    span,
                    format!(
                        "`extern module {}` must name a relative `.erl` path, `/`-separated, \
                         whose file name is a plain Erlang module name not starting with \
                         `{RESERVED_PREFIX}`",
                        token.text()
                    ),
                );
                continue;
            };
            if self
                .extern_modules
                .insert(String::from(module), span)
                .is_some()
            {
                let _ = self.error(
                    CheckCode::C0065,
                    span,
                    format!("Erlang module `{module}` is declared twice"),
                );
            }
        }
    }

    /// Elaborates an extern declaration's signature and binds it, checking
    /// its row (C0067) and its binding (C0064, C0065).
    pub(crate) fn declare_extern(&mut self, decl: &ExternDecl) -> Checked<()> {
        let Some(name) = decl.name() else {
            return Ok(());
        };
        let params: Vec<_> = decl.params().collect();
        let annotated = params.iter().all(|p| p.ty().is_some());
        let Some(ret) = decl.return_type() else {
            return Err(self.incomplete_extern(name, decl.syntax()));
        };
        if !annotated {
            return Err(self.incomplete_extern(name, decl.syntax()));
        }
        let ann = decl.effect_ann();
        let scheme = self.signature_scheme(&params, &ret, ann.as_ref())?;
        self.env.insert_root(name, scheme.clone());
        self.externs.insert(String::from(name));
        self.types
            .push((NodeKey::of_node(decl.syntax()), scheme.clone()));
        self.bindings.push((String::from(name), scheme.clone()));

        if let (Some(ann), Some(row)) = (&ann, scheme_row(&scheme))
            && !is_extern_row(row)
        {
            let _ = self.error(
                CheckCode::C0067,
                node_span(ann.syntax(), self.source_id),
                format!(
                    "extern `{name}` may declare at most one `Exn<…>` and nothing else \
                     (an Erlang function performs no Hirð effects), but its row is `{row}`"
                ),
            );
        }
        self.check_binding(name, decl);
        Ok(())
    }

    /// Checks an extern's `= "module":"function"`: present, both names plain
    /// atoms, the module declared by an `extern module` in this file.
    fn check_binding(&mut self, name: &str, decl: &ExternDecl) {
        let name_span = name_token_span(decl.syntax(), self.source_id);
        let Some(binding) = decl.binding() else {
            let _ = self.error(
                CheckCode::C0064,
                name_span,
                format!(
                    "extern `{name}` must name the Erlang function it binds: \
                     `= \"module\":\"function\"`"
                ),
            );
            return;
        };
        let (Some(module), Some(function)) = (binding.module_token(), binding.function_token())
        else {
            return;
        };
        if !literal(function).is_some_and(is_atom_name) {
            let _ = self.error(
                CheckCode::C0064,
                token_span(function, self.source_id),
                format!(
                    "extern `{name}` binds {}, which is not a plain Erlang function name \
                     (a lowercase letter, then letters, digits, and `_`)",
                    function.text()
                ),
            );
        }
        let declared = literal(module).is_some_and(|m| self.extern_modules.contains_key(m));
        if !declared {
            let _ = self.error(
                CheckCode::C0065,
                token_span(module, self.source_id),
                format!(
                    "extern `{name}` binds a function of {}, which no `extern module` in \
                     this file declares",
                    module.text()
                ),
            );
        }
    }

    /// Reports an extern declaration missing parts of its signature.
    fn incomplete_extern(&mut self, name: &str, node: &SyntaxNode) -> Aborted {
        let span = name_token_span(node, self.source_id);
        self.error(
            CheckCode::C0014,
            span,
            format!("extern `{name}` requires a fully annotated signature"),
        )
    }

    /// Whether `name` refers to a declared extern here: bound and not
    /// shadowed by an inner scope.
    pub(crate) fn is_extern_ref(&self, name: &str) -> bool {
        self.externs.contains(name) && self.env.resolves_to_root(name)
    }

    /// The type of a tool arm's handler when it is an extern, recorded for
    /// the node; `None` for any other handler. An extern's name is not a
    /// value, so this is the one place it resolves.
    pub(crate) fn extern_handler_type(&mut self, handler: &Expr) -> Option<Type> {
        let Expr::Name(name) = handler else {
            return None;
        };
        if !self.is_extern_ref(name.text()) {
            return None;
        }
        let scheme = self.env.lookup(name.text())?.clone();
        let ty = self.subst.instantiate(&scheme);
        self.types.push((NodeKey::of_expr(handler), ty.clone()));
        Some(ty)
    }

    /// Reports an extern arm whose `Exn<…>` the handled tool's declared row
    /// (`tool_row`, the instance the handler was unified with) lacks (C0068).
    pub(crate) fn check_extern_exn(
        &mut self,
        tool: &str,
        tool_row: &EffectRow,
        handler: &Expr,
        handler_ty: &Type,
    ) {
        let Type::TyFn(_, _, row) = self.subst.resolve(handler_ty) else {
            return;
        };
        let tool_row = self.subst.resolve_row(tool_row);
        let span = expr_span(handler, self.source_id);
        let Expr::Name(name) = handler else {
            return;
        };
        let name = name.text();
        for exn in row.effects() {
            if !tool_row.effects().any(|e| e == exn) {
                let _ = self.error(
                    CheckCode::C0068,
                    span,
                    format!(
                        "extern `{name}` may raise `{exn}`, but tool `{tool}` does not \
                         declare it; add `{exn}` to the tool's row"
                    ),
                );
            }
        }
    }
}

/// The row of a function type or scheme.
fn scheme_row(scheme: &Type) -> Option<&EffectRow> {
    let ty = match scheme {
        Type::TyForall(_, _, inner) => inner,
        other => other,
    };
    match ty {
        Type::TyFn(_, _, row) => Some(row),
        _ => None,
    }
}

/// Whether `row` is closed and holds at most one effect, an `Exn<…>`.
fn is_extern_row(row: &EffectRow) -> bool {
    let mut effects = row.effects();
    row.tail().is_none()
        && match (effects.next(), effects.next()) {
            (None, _) => true,
            (Some(e), None) => e.head().as_str() == "Exn",
            _ => false,
        }
}

/// A string literal's contents, without quotes; `None` when it carries an
/// escape, which no Erlang name or relative path needs.
fn literal(token: &SyntaxToken) -> Option<&str> {
    let text = token.text().strip_prefix('"')?.strip_suffix('"')?;
    (!text.contains('\\')).then_some(text)
}

/// The Erlang module an `extern module` path names: its file name without
/// `.erl`, when the path is relative and `/`-separated and the module a
/// plain, unreserved atom.
fn source_module(token: &SyntaxToken) -> Option<&str> {
    let path = literal(token)?;
    let file = path.rsplit('/').next()?;
    let module = file.strip_suffix(".erl")?;
    let relative = !path.starts_with('/') && path.split('/').all(|s| !s.is_empty());
    (relative && is_atom_name(module) && !module.starts_with(RESERVED_PREFIX)).then_some(module)
}

/// Whether `name` is a plain Erlang atom: a lowercase ASCII letter, then
/// ASCII letters, digits, and `_`.
fn is_atom_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Pretty-printing IR back to canonical Hirð source.
//!
//! [`pretty_print`] emits syntactically valid, canonically formatted source
//! for an [`IrModule`]. Re-parsing, re-checking, and re-lowering that source
//! reproduces the same IR up to type-variable renaming — the round-trip
//! property that guards lowering and inference against regressions.
//!
//! The printer is the inverse direction of lowering, so it re-introduces the
//! surface forms lowering erased:
//!
//! - Operator applications (`IrApp` whose callee is an operator reference)
//!   print infix (`a + b`), with parentheses inserted only where precedence or
//!   associativity demands them.
//! - `match` prints with its arms; the desugared `if` is gone, so a lowered
//!   `if` prints as the `match` over `Bool` it became (lossy by design).
//! - Function signatures print their parameter and return types. The empty
//!   effect row is elided, matching the surface convention that `! {}` is the
//!   default.
//!
//! Type variables in a signature are renumbered to `a, b, c, …` in order of
//! first appearance, so output is canonical regardless of the unification
//! variable identities inference happened to assign.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use core::fmt::{Display, Write as _};

use hird_types::{Effect, EffectRow, RowVar, Type};

use crate::ir::{
    IrApp, IrDecl, IrExpr, IrExternRef, IrFnDef, IrModule, IrPattern, IrTypeDef, LiteralValue,
};

/// Renders `module` as canonical Hirð source.
///
/// The output is parseable and, when re-checked and re-lowered, structurally
/// equal to `module` up to type-variable renaming.
#[must_use]
pub fn pretty_print(module: &IrModule) -> String {
    let mut printer = Printer { out: String::new() };
    printer.module(module);
    printer.out
}

// ── precedence ladder ────────────────────────────────────────────

/// Lowest binding: `let`, `λ`, and `match`, which extend to the right.
const PREC_LOW: u8 = 0;
/// Logical or (`∨`).
const PREC_OR: u8 = 1;
/// Logical and (`∧`).
const PREC_AND: u8 = 2;
/// Relational operators (`==`, `<`, …); non-associative.
const PREC_REL: u8 = 3;
/// Additive operators (`+`, `-`).
const PREC_ADD: u8 = 4;
/// Multiplicative operators (`*`, `/`).
const PREC_MUL: u8 = 5;
/// Function and constructor application.
const PREC_APP: u8 = 6;
/// Field access (`.`), the tightest-binding form below atoms.
const PREC_POSTFIX: u8 = 7;
/// A self-delimiting atom: literal, variable, tuple, list, or record.
const PREC_ATOM: u8 = 8;

/// Operator associativity, as far as the printer needs to parenthesise.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Assoc {
    /// Left-associative: a same-precedence operand needs no parentheses on the
    /// left, but does on the right.
    Left,
    /// Non-associative: a same-precedence operand needs parentheses on either
    /// side (the grammar rejects the unparenthesised chain).
    NonAssoc,
}

/// The precedence and associativity of a binary operator reference, or `None`
/// for any other name.
fn operator_info(name: &str) -> Option<(u8, Assoc)> {
    Some(match name {
        "\u{2228}" => (PREC_OR, Assoc::Left),
        "\u{2227}" => (PREC_AND, Assoc::Left),
        "==" | "!=" | "<" | ">" | "<=" | ">=" => (PREC_REL, Assoc::NonAssoc),
        "+" | "-" => (PREC_ADD, Assoc::Left),
        "*" | "/" => (PREC_MUL, Assoc::Left),
        _ => return None,
    })
}

/// The operator name, precedence, and associativity of an application that
/// lowering produced from an infix operator, or `None` for an ordinary call.
fn as_operator(app: &IrApp) -> Option<(&str, u8, Assoc)> {
    if app.args.len() != 2 {
        return None;
    }
    let IrExpr::Var(var) = app.func.as_ref() else {
        return None;
    };
    operator_info(&var.name).map(|(prec, assoc)| (var.name.as_str(), prec, assoc))
}

/// The precedence of an expression, used to decide when a child needs
/// parentheses.
fn expr_prec(expr: &IrExpr) -> u8 {
    match expr {
        IrExpr::Literal(_)
        | IrExpr::Var(_)
        | IrExpr::Tuple(_)
        | IrExpr::List(_)
        | IrExpr::Record(_) => PREC_ATOM,
        IrExpr::Field(_) => PREC_POSTFIX,
        IrExpr::Constructor(ctor) => {
            if ctor.args.is_empty() {
                PREC_ATOM
            } else {
                PREC_APP
            }
        }
        IrExpr::Lambda(_) | IrExpr::Let(_) | IrExpr::Match(_) => PREC_LOW,
        IrExpr::App(app) => match as_operator(app) {
            Some((_, prec, _)) => prec,
            None => PREC_APP,
        },
    }
}

// ── type expressibility ──────────────────────────────────────────

/// Whether `ty` can be written as a surface type expression. Some types have
/// no annotation syntax, so a function whose return type is one of them must
/// omit its (optional) return annotation and let inference recover it:
///
/// - records and unit (`()`), which the type grammar has no form for;
/// - a zero-argument function `() → T`, whose `()` is not a valid operand —
///   it would re-parse as the one-argument `(()) → T`.
fn is_expressible(ty: &Type) -> bool {
    match ty {
        Type::TyVar(_) => true,
        Type::TyCon(_, args) => args.iter().all(is_expressible),
        Type::TyFn(params, ret, _) => {
            !params.is_empty() && params.iter().all(is_expressible) && is_expressible(ret)
        }
        // A 2+-tuple is expressible; unit (and the non-occurring 1-tuple) is not.
        Type::TyTuple(elems) => elems.len() >= 2 && elems.iter().all(is_expressible),
        Type::TyRecord(_) => false,
        Type::TyForall(_, _, body) => is_expressible(body),
    }
}

// ── type-variable canonicalisation ───────────────────────────────

/// Identity of a type variable for canonical renumbering: a unification
/// variable index or a skolem name (a lowercase, hence unutterable,
/// constructor that stands for a rigid signature variable).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum VarKey {
    /// A unification variable, by index.
    Unif(u32),
    /// A skolem constant, by name.
    Skolem(String),
}

/// A per-signature map assigning each type variable its canonical index.
type VarMap = BTreeMap<VarKey, u32>;

/// A per-signature map assigning each row variable its canonical index, in a
/// sequence independent of the type variables' (so they render with their own
/// `r, r1, …` letters).
type RowMap = BTreeMap<RowVar, u32>;

/// The canonical index for `key`, allocating the next one on first sight.
fn intern(map: &mut VarMap, key: VarKey) -> u32 {
    let next = u32::try_from(map.len()).unwrap_or(u32::MAX);
    *map.entry(key).or_insert(next)
}

/// The canonical index for row variable `var`, allocating the next one on first
/// sight.
fn intern_row(map: &mut RowMap, var: RowVar) -> u32 {
    let next = u32::try_from(map.len()).unwrap_or(u32::MAX);
    *map.entry(var).or_insert(next)
}

/// Whether a type name denotes a variable rather than a constructor. The lexer
/// reserves lowercase for type variables and `PascalCase` for constructors, so
/// a lowercase name is always a variable (a written one or an internal
/// skolem).
fn is_type_var(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_lowercase)
}

/// A copy of `ty` with every type variable (unification variable or skolem)
/// rewritten to a [`Type::TyVar`] numbered by first appearance through `vars`,
/// and every row variable renumbered through `rows`, so it renders with
/// canonical `a, b, c, …` and `r, r1, …` letters.
fn canonical_type(ty: &Type, vars: &mut VarMap, rows: &mut RowMap) -> Type {
    match ty {
        Type::TyVar(id) => Type::TyVar(intern(vars, VarKey::Unif(*id))),
        Type::TyCon(name, args) if args.is_empty() && is_type_var(name.as_str()) => {
            Type::TyVar(intern(vars, VarKey::Skolem(String::from(name.as_str()))))
        }
        Type::TyCon(name, args) => Type::TyCon(
            name.clone(),
            args.iter().map(|a| canonical_type(a, vars, rows)).collect(),
        ),
        Type::TyFn(params, ret, row) => Type::TyFn(
            params
                .iter()
                .map(|p| canonical_type(p, vars, rows))
                .collect(),
            Box::new(canonical_type(ret, vars, rows)),
            canonical_effect_row(row, vars, rows),
        ),
        Type::TyTuple(elems) => Type::TyTuple(
            elems
                .iter()
                .map(|e| canonical_type(e, vars, rows))
                .collect(),
        ),
        Type::TyRecord(fields) => Type::TyRecord(
            fields
                .iter()
                .map(|(label, v)| (label.clone(), canonical_type(v, vars, rows)))
                .collect(),
        ),
        Type::TyForall(tvars, rvars, body) => Type::TyForall(
            tvars
                .iter()
                .map(|v| intern(vars, VarKey::Unif(*v)))
                .collect(),
            rvars
                .iter()
                .map(|v| RowVar::new(intern_row(rows, *v)))
                .collect(),
            Box::new(canonical_type(body, vars, rows)),
        ),
    }
}

/// A copy of `row` with its effects' type arguments canonicalised through
/// `vars`/`rows` and its tail row variable renumbered through `rows`.
fn canonical_effect_row(row: &EffectRow, vars: &mut VarMap, rows: &mut RowMap) -> EffectRow {
    let mut out = EffectRow::empty();
    for effect in row.effects() {
        out.insert(canonical_effect(effect, vars, rows));
    }
    out.with_tail(row.tail().map(|rv| RowVar::new(intern_row(rows, rv))))
}

/// A copy of `effect` with its type arguments canonicalised.
fn canonical_effect(effect: &Effect, vars: &mut VarMap, rows: &mut RowMap) -> Effect {
    match effect {
        Effect::Named(name) => Effect::Named(name.clone()),
        Effect::Parametric(name, args) => Effect::Parametric(
            name.clone(),
            args.iter().map(|a| canonical_type(a, vars, rows)).collect(),
        ),
    }
}

/// The source text of a literal value (strings keep their surrounding quotes).
fn literal_text(value: &LiteralValue) -> &str {
    match value {
        LiteralValue::Int(text) | LiteralValue::Float(text) | LiteralValue::Str(text) => text,
    }
}

// ── effect-declaration synthesis ──────────────────────────────────

/// Every effect the printer will emit, mapped from head name to type-argument
/// count, collected from the declaration-level types it renders (function
/// signatures, extern types, and constructor fields). Held name-sorted so the
/// synthesised declarations print deterministically.
fn collect_effects(module: &IrModule) -> BTreeMap<String, usize> {
    let mut effects = BTreeMap::new();
    for decl in &module.declarations {
        match decl {
            IrDecl::Fn(f) => {
                for param in &f.params {
                    collect_type_effects(&param.ty, &mut effects);
                }
                collect_type_effects(&f.return_type, &mut effects);
                collect_row_effects(&f.effect_row, &mut effects);
            }
            IrDecl::Extern(e) => collect_type_effects(&e.ty, &mut effects),
            IrDecl::Type(t) => {
                for ctor in &t.constructors {
                    for field in &ctor.fields {
                        collect_type_effects(field, &mut effects);
                    }
                }
            }
        }
    }
    effects
}

/// Accumulates the effects in every function row reachable from `ty`.
fn collect_type_effects(ty: &Type, out: &mut BTreeMap<String, usize>) {
    match ty {
        Type::TyVar(_) => {}
        Type::TyCon(_, args) => {
            for arg in args {
                collect_type_effects(arg, out);
            }
        }
        Type::TyFn(params, ret, row) => {
            for param in params {
                collect_type_effects(param, out);
            }
            collect_type_effects(ret, out);
            collect_row_effects(row, out);
        }
        Type::TyTuple(elems) => {
            for elem in elems {
                collect_type_effects(elem, out);
            }
        }
        Type::TyRecord(fields) => {
            for value in fields.values() {
                collect_type_effects(value, out);
            }
        }
        Type::TyForall(_, _, body) => collect_type_effects(body, out),
    }
}

/// Accumulates `row`'s effects (head and arity) plus any effects nested in
/// their type arguments.
fn collect_row_effects(row: &EffectRow, out: &mut BTreeMap<String, usize>) {
    for effect in row.effects() {
        out.insert(String::from(effect.head().as_str()), effect.args().len());
        for arg in effect.args() {
            collect_type_effects(arg, out);
        }
    }
}

// ── the printer ──────────────────────────────────────────────────

/// Accumulates rendered source.
struct Printer {
    /// The output buffer.
    out: String,
}

impl Printer {
    /// Appends a string slice verbatim.
    fn push(&mut self, s: &str) {
        self.out.push_str(s);
    }

    /// Appends a value through its [`Display`] implementation.
    fn push_display(&mut self, value: &impl Display) {
        // Writing into a `String` is infallible.
        let _ = write!(self.out, "{value}");
    }

    /// Appends a type, canonicalising its type and row variables through the
    /// per-signature `vars`/`rows` maps.
    fn push_type(&mut self, ty: &Type, vars: &mut VarMap, rows: &mut RowMap) {
        self.push_display(&canonical_type(ty, vars, rows));
    }

    /// Renders a module: its name, synthesised `effect` declarations for every
    /// effect the body references, then the declarations separated by blank
    /// lines.
    ///
    /// Effect declarations are not IR nodes (only the rows on functions are),
    /// so the printer reconstructs them from usage. Without this the printed
    /// source would name effects it never declares and fail to re-check,
    /// breaking the round-trip property.
    fn module(&mut self, module: &IrModule) {
        self.push("module ");
        self.push(&module.name);
        self.push("\n");
        for (head, arity) in collect_effects(module) {
            self.push("\n");
            self.effect_decl(&head, arity);
            self.push("\n");
        }
        for decl in &module.declarations {
            self.push("\n");
            match decl {
                IrDecl::Fn(f) => self.fn_def(f),
                IrDecl::Type(t) => self.type_def(t),
                IrDecl::Extern(e) => self.extern_ref(e),
            }
            self.push("\n");
        }
    }

    /// `effect Head` or `effect Head<t0, …>`. Parameter names are synthesised
    /// (the IR records only the arity, which is all re-checking needs).
    fn effect_decl(&mut self, head: &str, arity: usize) {
        self.push("effect ");
        self.push(head);
        if arity > 0 {
            self.push("<");
            for i in 0..arity {
                if i > 0 {
                    self.push(", ");
                }
                let _ = write!(self.out, "t{i}");
            }
            self.push(">");
        }
    }

    /// `fn name(params) → ret = body`. The return annotation is omitted when
    /// the type is not expressible (a record or unit), and the empty effect
    /// row is elided.
    fn fn_def(&mut self, f: &IrFnDef) {
        let mut vars = VarMap::new();
        let mut rows = RowMap::new();
        self.push("fn ");
        self.push(&f.name);
        self.push("(");
        for (i, param) in f.params.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.push(&param.name);
            self.push(": ");
            self.push_type(&param.ty, &mut vars, &mut rows);
        }
        self.push(")");
        if is_expressible(&f.return_type) {
            self.push(" \u{2192} ");
            self.push_type(&f.return_type, &mut vars, &mut rows);
        }
        // A non-empty effect row prints after the return type, sharing the
        // signature's variable numbering so a row variable that also appears in
        // a parameter type renders with the same letter.
        if !f.effect_row.is_empty() {
            self.push(" ! ");
            self.push_display(&canonical_effect_row(&f.effect_row, &mut vars, &mut rows));
        }
        self.push(" = ");
        self.expr(&f.body, PREC_LOW);
    }

    /// `type Name<params> = C1(fields) | C2 | …`. Constructor field types are
    /// rendered under the declared parameter names, so no canonicalisation is
    /// applied here.
    fn type_def(&mut self, t: &IrTypeDef) {
        self.push("type ");
        self.push(&t.name);
        if let [first, rest @ ..] = t.params.as_slice() {
            self.push("<");
            self.push(first);
            for param in rest {
                self.push(", ");
                self.push(param);
            }
            self.push(">");
        }
        self.push(" = ");
        for (i, ctor) in t.constructors.iter().enumerate() {
            if i > 0 {
                self.push(" | ");
            }
            self.push(&ctor.name);
            if let [first, rest @ ..] = ctor.fields.as_slice() {
                self.push("(");
                self.push_display(first);
                for field in rest {
                    self.push(", ");
                    self.push_display(field);
                }
                self.push(")");
            }
        }
    }

    /// `extern fn name(params) → ret`. Parameter names are synthesised (the IR
    /// keeps only the signature type); the return annotation is mandatory, as
    /// the surface grammar requires.
    fn extern_ref(&mut self, e: &IrExternRef) {
        let mut vars = VarMap::new();
        let mut rows = RowMap::new();
        self.push("extern fn ");
        self.push(&e.name);
        self.push("(");
        let body = match &e.ty {
            Type::TyForall(_, _, inner) => inner.as_ref(),
            other => other,
        };
        let ret = match body {
            Type::TyFn(params, ret, _) => {
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    let _ = write!(self.out, "p{i}");
                    self.push(": ");
                    self.push_type(param, &mut vars, &mut rows);
                }
                ret.as_ref()
            }
            // Defensive: a non-function extern type is not reachable from a
            // valid declaration, which always has a parameter list.
            other => other,
        };
        self.push(")");
        self.push(" \u{2192} ");
        self.push_type(ret, &mut vars, &mut rows);
    }

    /// Renders `expr`, wrapping it in parentheses when its precedence is below
    /// what the surrounding position requires.
    fn expr(&mut self, expr: &IrExpr, min_prec: u8) {
        let parens = expr_prec(expr) < min_prec;
        if parens {
            self.push("(");
        }
        self.naked(expr);
        if parens {
            self.push(")");
        }
    }

    /// Renders `expr` without any outer parentheses, delegating child
    /// parenthesisation to [`Self::expr`].
    fn naked(&mut self, expr: &IrExpr) {
        match expr {
            IrExpr::Literal(lit) => self.push(literal_text(&lit.value)),
            IrExpr::Var(var) => self.push(&var.name),
            IrExpr::Tuple(tuple) => {
                self.push("(");
                self.comma_separated(&tuple.elems);
                self.push(")");
            }
            IrExpr::List(list) => {
                self.push("[");
                self.comma_separated(&list.elems);
                self.push("]");
            }
            IrExpr::Record(record) => {
                if record.fields.is_empty() {
                    self.push("{}");
                } else {
                    self.push("{ ");
                    for (i, field) in record.fields.iter().enumerate() {
                        if i > 0 {
                            self.push(", ");
                        }
                        self.push(&field.label);
                        self.push(": ");
                        self.expr(&field.value, PREC_LOW);
                    }
                    self.push(" }");
                }
            }
            IrExpr::Field(field) => {
                self.expr(&field.receiver, PREC_APP);
                self.push(".");
                self.push(&field.field);
            }
            IrExpr::Constructor(ctor) => {
                self.push(&ctor.name);
                if !ctor.args.is_empty() {
                    self.push("(");
                    self.comma_separated(&ctor.args);
                    self.push(")");
                }
            }
            IrExpr::Lambda(lambda) => {
                self.push("\u{3bb}");
                for (i, param) in lambda.params.iter().enumerate() {
                    if i > 0 {
                        self.push(" ");
                    }
                    self.push(&param.name);
                }
                self.push(" \u{2192} ");
                self.expr(&lambda.body, PREC_LOW);
            }
            IrExpr::Let(le) => {
                self.push("let ");
                self.push(&le.name);
                self.push(" = ");
                self.expr(&le.value, PREC_LOW);
                self.push(" in ");
                self.expr(&le.body, PREC_LOW);
            }
            IrExpr::Match(m) => {
                self.push("match ");
                self.expr(&m.scrutinee, PREC_LOW);
                self.push(" { ");
                for (i, arm) in m.arms.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.pattern(&arm.pattern);
                    self.push(" \u{2192} ");
                    self.expr(&arm.body, PREC_LOW);
                }
                self.push(" }");
            }
            IrExpr::App(app) => match as_operator(app) {
                Some((op, prec, assoc)) => {
                    let (left_min, right_min) = match assoc {
                        Assoc::Left => (prec, prec + 1),
                        Assoc::NonAssoc => (prec + 1, prec + 1),
                    };
                    self.expr(&app.args[0], left_min);
                    self.push(" ");
                    self.push(op);
                    self.push(" ");
                    self.expr(&app.args[1], right_min);
                }
                None => {
                    self.expr(&app.func, PREC_APP);
                    self.push("(");
                    self.comma_separated(&app.args);
                    self.push(")");
                }
            },
        }
    }

    /// Renders expressions joined by `, `, each at the lowest precedence (a
    /// delimited position needs no parentheses).
    fn comma_separated(&mut self, exprs: &[IrExpr]) {
        for (i, expr) in exprs.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.expr(expr, PREC_LOW);
        }
    }

    /// Renders a pattern. Patterns are self-delimiting, so no precedence
    /// tracking is needed.
    fn pattern(&mut self, pattern: &IrPattern) {
        match pattern {
            IrPattern::Wildcard(_) => self.push("_"),
            IrPattern::Bind(bind) => self.push(&bind.name),
            IrPattern::Literal(lit) => self.push(literal_text(&lit.value)),
            IrPattern::Tuple(tuple) => {
                self.push("(");
                for (i, elem) in tuple.elems.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    self.pattern(elem);
                }
                self.push(")");
            }
            IrPattern::Constructor(ctor) => {
                self.push(&ctor.name);
                if let [first, rest @ ..] = ctor.fields.as_slice() {
                    self.push("(");
                    self.pattern(first);
                    for field in rest {
                        self.push(", ");
                        self.pattern(field);
                    }
                    self.push(")");
                }
            }
        }
    }
}

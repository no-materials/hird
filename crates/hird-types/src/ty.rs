// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The semantic type representation and its human-readable rendering.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt;

use crate::effect::{EffectRow, RowVar};
use crate::name::{Label, Name};

/// A semantic type.
///
/// Built-in constructors (`Int`, `Float`, `String`, `Bool`, `List`,
/// `Option`) are not distinct variants: they are ordinary [`Type::TyCon`]
/// values whose names happen to be reserved, so they render and unify
/// through the generic constructor path.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Type {
    /// Unification variable, identified by index into a [`crate::Subst`].
    TyVar(u32),
    /// Type constructor applied to zero or more arguments.
    TyCon(Name, Vec<Self>),
    /// Function from a parameter list to a result, carrying the effect row the
    /// function performs. Arity is semantic — BEAM functions are n-ary and
    /// there is no auto-currying — so `(A, B) → C` and `A → (B → C)` are
    /// distinct types. A pure function carries the empty row.
    TyFn(Vec<Self>, Box<Self>, EffectRow),
    /// Anonymous tuple.
    TyTuple(Vec<Self>),
    /// Structural record, keyed by label and held label-sorted.
    TyRecord(BTreeMap<Label, Self>),
    /// Type quantified over the listed type variables and row variables.
    /// Produced by generalisation and consumed by instantiation; both are later
    /// passes, so this variant is represented but never unified directly.
    TyForall(Vec<u32>, Vec<RowVar>, Box<Self>),
}

impl Type {
    /// A unification variable.
    #[must_use]
    pub const fn var(id: u32) -> Self {
        Self::TyVar(id)
    }

    /// A constructor applied to `args`.
    #[must_use]
    pub fn con(name: impl Into<Name>, args: Vec<Self>) -> Self {
        Self::TyCon(name.into(), args)
    }

    /// A pure function type `params -> ret` of arity `params.len()` (empty
    /// effect row).
    #[must_use]
    pub fn func(params: Vec<Self>, ret: Self) -> Self {
        Self::TyFn(params, Box::new(ret), EffectRow::empty())
    }

    /// A function type `params -> ret ! row` of arity `params.len()`.
    #[must_use]
    pub fn func_eff(params: Vec<Self>, ret: Self, row: EffectRow) -> Self {
        Self::TyFn(params, Box::new(ret), row)
    }

    /// A tuple of the given element types.
    #[must_use]
    pub fn tuple(elems: Vec<Self>) -> Self {
        Self::TyTuple(elems)
    }

    /// A record from `(label, type)` pairs. Later pairs override earlier ones
    /// sharing a label.
    #[must_use]
    pub fn record(fields: impl IntoIterator<Item = (Label, Self)>) -> Self {
        Self::TyRecord(fields.into_iter().collect())
    }

    /// The built-in `Int`.
    #[must_use]
    pub fn int() -> Self {
        Self::con("Int", Vec::new())
    }

    /// The built-in `Float`.
    #[must_use]
    pub fn float() -> Self {
        Self::con("Float", Vec::new())
    }

    /// The built-in `String`.
    #[must_use]
    pub fn string() -> Self {
        Self::con("String", Vec::new())
    }

    /// The built-in `Bool`.
    #[must_use]
    pub fn bool() -> Self {
        Self::con("Bool", Vec::new())
    }

    /// The built-in `List<elem>`.
    #[must_use]
    pub fn list(elem: Self) -> Self {
        Self::con("List", Vec::from([elem]))
    }

    /// The built-in `Option<inner>`.
    #[must_use]
    pub fn option(inner: Self) -> Self {
        Self::con("Option", Vec::from([inner]))
    }

    /// Structurally substitutes the type variables in `types` and the row
    /// variables in `rows`, leaving every other node unchanged. Quantifier
    /// bodies are closed under their binders, so no capture occurs.
    ///
    /// Substitution descends through function effect rows: the type arguments
    /// of a parametric effect are rewritten through `types`, and an open row's
    /// tail is rewritten through `rows`. This is how instantiation refreshes a
    /// quantified row variable.
    #[must_use]
    pub fn substitute(&self, types: &BTreeMap<u32, Self>, rows: &BTreeMap<RowVar, RowVar>) -> Self {
        match self {
            Self::TyVar(v) => types.get(v).cloned().unwrap_or_else(|| self.clone()),
            Self::TyCon(name, args) => Self::TyCon(
                name.clone(),
                args.iter().map(|a| a.substitute(types, rows)).collect(),
            ),
            Self::TyFn(params, ret, row) => Self::TyFn(
                params.iter().map(|p| p.substitute(types, rows)).collect(),
                Box::new(ret.substitute(types, rows)),
                substitute_row(row, types, rows),
            ),
            Self::TyTuple(elems) => {
                Self::TyTuple(elems.iter().map(|e| e.substitute(types, rows)).collect())
            }
            Self::TyRecord(fields) => Self::TyRecord(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.substitute(types, rows)))
                    .collect(),
            ),
            Self::TyForall(tvars, rvars, body) => Self::TyForall(
                tvars.clone(),
                rvars.clone(),
                Box::new(body.substitute(types, rows)),
            ),
        }
    }

    /// A clone with variables renumbered densely from `0` in order of first
    /// appearance ([`Type::TyForall`] binders first), so equivalent types
    /// render identically (`∀a. a → a` rather than `∀c7. c7 → c7`).
    ///
    /// For display only: renumbering does not preserve variable identity
    /// across distinct types.
    #[must_use]
    pub fn normalized(&self) -> Self {
        let mut types = BTreeMap::new();
        let mut rows = BTreeMap::new();
        self.rename(&mut types, &mut rows)
    }

    /// Rewrites type variables through `types` and row variables through `rows`,
    /// assigning the next dense id to each variable not yet mapped. The two
    /// kinds renumber in independent sequences, so a type variable and a row
    /// variable never share an id (and so render with their own letters).
    fn rename(&self, types: &mut BTreeMap<u32, u32>, rows: &mut BTreeMap<u32, u32>) -> Self {
        match self {
            Self::TyVar(v) => Self::TyVar(renumber(types, *v)),
            Self::TyCon(name, args) => Self::TyCon(
                name.clone(),
                args.iter().map(|a| a.rename(types, rows)).collect(),
            ),
            Self::TyFn(params, ret, row) => Self::TyFn(
                params.iter().map(|p| p.rename(types, rows)).collect(),
                Box::new(ret.rename(types, rows)),
                rename_row(row, types, rows),
            ),
            Self::TyTuple(elems) => {
                Self::TyTuple(elems.iter().map(|e| e.rename(types, rows)).collect())
            }
            Self::TyRecord(fields) => Self::TyRecord(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.rename(types, rows)))
                    .collect(),
            ),
            Self::TyForall(tvars, rvars, body) => {
                let tvars = tvars.iter().map(|v| renumber(types, *v)).collect();
                let rvars = rvars
                    .iter()
                    .map(|v| RowVar::new(renumber(rows, v.index())))
                    .collect();
                Self::TyForall(tvars, rvars, Box::new(body.rename(types, rows)))
            }
        }
    }

    /// Renders `self`, parenthesising a bare function type when it appears as
    /// an operand of an enclosing arrow chain (an unparenthesised chain
    /// denotes a single n-ary function, so nested functions on either side
    /// need parentheses).
    fn write(&self, f: &mut fmt::Formatter<'_>, fn_operand: bool) -> fmt::Result {
        match self {
            Self::TyVar(id) => write_var(f, *id),
            Self::TyCon(name, args) => {
                fmt::Display::fmt(name, f)?;
                if let [first, rest @ ..] = args.as_slice() {
                    f.write_str("<")?;
                    first.write(f, false)?;
                    for arg in rest {
                        f.write_str(", ")?;
                        arg.write(f, false)?;
                    }
                    f.write_str(">")?;
                }
                Ok(())
            }
            Self::TyFn(params, ret, row) => {
                if fn_operand {
                    f.write_str("(")?;
                }
                if params.is_empty() {
                    // A 0-ary function; `()` here is not the unit tuple.
                    f.write_str("()")?;
                } else {
                    for (i, param) in params.iter().enumerate() {
                        if i > 0 {
                            f.write_str(" \u{2192} ")?;
                        }
                        param.write(f, true)?;
                    }
                }
                f.write_str(" \u{2192} ")?;
                ret.write(f, true)?;
                // The empty row is elided; a non-empty one prints `! {…}`.
                if !row.is_empty() {
                    f.write_str(" ! ")?;
                    fmt::Display::fmt(row, f)?;
                }
                if fn_operand {
                    f.write_str(")")?;
                }
                Ok(())
            }
            Self::TyTuple(elems) => {
                f.write_str("(")?;
                for (i, elem) in elems.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    elem.write(f, false)?;
                }
                f.write_str(")")
            }
            Self::TyRecord(fields) => {
                if fields.is_empty() {
                    return f.write_str("{}");
                }
                f.write_str("{ ")?;
                for (i, (label, ty)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    fmt::Display::fmt(label, f)?;
                    f.write_str(": ")?;
                    ty.write(f, false)?;
                }
                f.write_str(" }")
            }
            Self::TyForall(tvars, rvars, body) => {
                f.write_str("\u{2200}")?;
                let mut first = true;
                for var in tvars {
                    if !first {
                        f.write_str(" ")?;
                    }
                    first = false;
                    write_var(f, *var)?;
                }
                for var in rvars {
                    if !first {
                        f.write_str(" ")?;
                    }
                    first = false;
                    fmt::Display::fmt(var, f)?;
                }
                f.write_str(". ")?;
                body.write(f, false)
            }
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, false)
    }
}

/// The dense id for `var`, allocating the next unused one on first sight.
fn renumber(map: &mut BTreeMap<u32, u32>, var: u32) -> u32 {
    let next = u32::try_from(map.len()).unwrap_or(u32::MAX);
    *map.entry(var).or_insert(next)
}

/// Substitutes a function's effect row: each parametric effect's type arguments
/// through `types`, and the tail row variable through `rows`. Named effects and
/// a closed tail pass through unchanged.
fn substitute_row(
    row: &EffectRow,
    types: &BTreeMap<u32, Type>,
    rows: &BTreeMap<RowVar, RowVar>,
) -> EffectRow {
    let mut out = EffectRow::empty();
    for effect in row.effects() {
        out.insert(effect.map_args(|a| a.substitute(types, rows)));
    }
    out.with_tail(row.tail().map(|rv| rows.get(&rv).copied().unwrap_or(rv)))
}

/// Renumbers a function's effect row for canonical display: each parametric
/// effect's type arguments through `types`, and the tail row variable through
/// the independent `rows` sequence.
fn rename_row(
    row: &EffectRow,
    types: &mut BTreeMap<u32, u32>,
    rows: &mut BTreeMap<u32, u32>,
) -> EffectRow {
    let mut out = EffectRow::empty();
    for effect in row.effects() {
        out.insert(effect.map_args(|a| a.rename(types, rows)));
    }
    out.with_tail(row.tail().map(|rv| RowVar::new(renumber(rows, rv.index()))))
}

/// Renders variable `id` as a lowercase letter, suffixing a counter once the
/// alphabet is exhausted (`a`, `b`, …, `z`, `a1`, `b1`, …). The mapping is a
/// bijection, so distinct variables never collide.
fn write_var(f: &mut fmt::Formatter<'_>, id: u32) -> fmt::Result {
    // `id % 26` is in `0..26`, so the conversion and 1-byte ASCII slice are
    // always in bounds.
    let idx = usize::try_from(id % 26).unwrap_or(0);
    f.write_str(&"abcdefghijklmnopqrstuvwxyz"[idx..idx + 1])?;
    let suffix = id / 26;
    if suffix > 0 {
        write!(f, "{suffix}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::vec;

    use super::Type;
    use crate::name::Label;

    // -- variable rendering ------------------------------------------------

    #[test]
    fn var_renders_as_letter() {
        assert_eq!(format!("{}", Type::var(0)), "a");
        assert_eq!(format!("{}", Type::var(1)), "b");
        assert_eq!(format!("{}", Type::var(25)), "z");
    }

    #[test]
    fn var_suffixes_past_the_alphabet() {
        assert_eq!(format!("{}", Type::var(26)), "a1");
        assert_eq!(format!("{}", Type::var(27)), "b1");
        assert_eq!(format!("{}", Type::var(52)), "a2");
    }

    // -- constructors ------------------------------------------------------

    #[test]
    fn nullary_builtins_render_bare() {
        assert_eq!(format!("{}", Type::int()), "Int");
        assert_eq!(format!("{}", Type::bool()), "Bool");
        assert_eq!(format!("{}", Type::string()), "String");
    }

    #[test]
    fn applied_constructors_use_angle_brackets() {
        assert_eq!(format!("{}", Type::list(Type::int())), "List<Int>");
        assert_eq!(format!("{}", Type::option(Type::var(0))), "Option<a>");
    }

    #[test]
    fn nested_constructors_nest() {
        let ty = Type::list(Type::option(Type::var(0)));
        assert_eq!(format!("{ty}"), "List<Option<a>>");
    }

    #[test]
    fn multi_argument_constructor() {
        let ty = Type::con("Map", vec![Type::string(), Type::int()]);
        assert_eq!(format!("{ty}"), "Map<String, Int>");
    }

    // -- functions ---------------------------------------------------------

    #[test]
    fn function_uses_unicode_arrow() {
        let ty = Type::func(vec![Type::var(0)], Type::var(1));
        assert_eq!(format!("{ty}"), "a \u{2192} b");
    }

    #[test]
    fn nary_function_renders_as_flat_chain() {
        let ty = Type::func(vec![Type::var(0), Type::var(1)], Type::var(2));
        assert_eq!(format!("{ty}"), "a \u{2192} b \u{2192} c");
    }

    #[test]
    fn zero_ary_function_renders_unit_params() {
        let ty = Type::func(vec![], Type::int());
        assert_eq!(format!("{ty}"), "() \u{2192} Int");
    }

    #[test]
    fn function_parameter_is_parenthesised() {
        let ty = Type::func(
            vec![Type::func(vec![Type::var(0)], Type::var(1))],
            Type::var(2),
        );
        assert_eq!(format!("{ty}"), "(a \u{2192} b) \u{2192} c");
    }

    #[test]
    fn function_return_is_parenthesised() {
        let ty = Type::func(
            vec![Type::var(0)],
            Type::func(vec![Type::var(1)], Type::var(2)),
        );
        assert_eq!(format!("{ty}"), "a \u{2192} (b \u{2192} c)");
    }

    // -- tuples ------------------------------------------------------------

    #[test]
    fn tuple_renders_parenthesised() {
        let ty = Type::tuple(vec![Type::int(), Type::string()]);
        assert_eq!(format!("{ty}"), "(Int, String)");
    }

    // -- records -----------------------------------------------------------

    #[test]
    fn record_renders_label_sorted() {
        // Inserted out of order; display must sort by label.
        let ty = Type::record([
            (Label::new("name"), Type::string()),
            (Label::new("age"), Type::int()),
        ]);
        assert_eq!(format!("{ty}"), "{ age: Int, name: String }");
    }

    #[test]
    fn empty_record_renders_braces() {
        assert_eq!(format!("{}", Type::record([])), "{}");
    }

    // -- quantified --------------------------------------------------------

    #[test]
    fn forall_renders_with_binder() {
        let body = Type::func(vec![Type::var(0)], Type::var(1));
        let ty = Type::TyForall(vec![0], vec![], alloc::boxed::Box::new(body));
        assert_eq!(format!("{ty}"), "\u{2200}a. a \u{2192} b");
    }

    // -- normalisation -------------------------------------------------------

    #[test]
    fn normalized_renumbers_in_first_appearance_order() {
        let ty = Type::func(vec![Type::var(7), Type::var(3)], Type::var(7));
        assert_eq!(format!("{}", ty.normalized()), "a \u{2192} b \u{2192} a");
    }

    #[test]
    fn normalized_assigns_forall_binders_first() {
        let body = Type::func(vec![Type::var(9)], Type::var(4));
        let ty = Type::TyForall(vec![9], vec![], alloc::boxed::Box::new(body));
        assert_eq!(format!("{}", ty.normalized()), "\u{2200}a. a \u{2192} b");
    }
}

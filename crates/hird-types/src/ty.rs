// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The semantic type representation and its human-readable rendering.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::fmt;

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
    /// Function from one type to another.
    TyFn(Box<Self>, Box<Self>),
    /// Anonymous tuple.
    TyTuple(Vec<Self>),
    /// Structural record, keyed by label and held label-sorted.
    TyRecord(BTreeMap<Label, Self>),
    /// Type quantified over the listed variables. Produced by generalisation
    /// and consumed by instantiation; both are later passes, so this variant
    /// is represented but never unified directly.
    TyForall(Vec<u32>, Box<Self>),
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

    /// A function type `from -> to`.
    #[must_use]
    pub fn func(from: Self, to: Self) -> Self {
        Self::TyFn(Box::new(from), Box::new(to))
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

    /// Renders `self`, parenthesising a bare function type when it appears as
    /// the left operand of an enclosing arrow (which is right-associative).
    fn write(&self, f: &mut fmt::Formatter<'_>, fn_left: bool) -> fmt::Result {
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
            Self::TyFn(from, to) => {
                if fn_left {
                    f.write_str("(")?;
                }
                from.write(f, true)?;
                f.write_str(" \u{2192} ")?;
                to.write(f, false)?;
                if fn_left {
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
            Self::TyForall(vars, body) => {
                f.write_str("\u{2200}")?;
                for (i, var) in vars.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" ")?;
                    }
                    write_var(f, *var)?;
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
        let ty = Type::func(Type::var(0), Type::var(1));
        assert_eq!(format!("{ty}"), "a \u{2192} b");
    }

    #[test]
    fn arrow_is_right_associative() {
        let ty = Type::func(Type::var(0), Type::func(Type::var(1), Type::var(2)));
        assert_eq!(format!("{ty}"), "a \u{2192} b \u{2192} c");
    }

    #[test]
    fn function_on_the_left_is_parenthesised() {
        let ty = Type::func(Type::func(Type::var(0), Type::var(1)), Type::var(2));
        assert_eq!(format!("{ty}"), "(a \u{2192} b) \u{2192} c");
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
        let body = Type::func(Type::var(0), Type::var(1));
        let ty = Type::TyForall(vec![0], alloc::boxed::Box::new(body));
        assert_eq!(format!("{ty}"), "\u{2200}a. a \u{2192} b");
    }
}

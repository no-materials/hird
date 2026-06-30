// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Effect rows: the type-level summary of what a function does.
//!
//! A function type carries an [`EffectRow`] alongside its parameters and
//! result. A row is an idempotent set of [`Effect`]s with an optional tail
//! [`RowVar`]: a closed row (`None` tail) is exactly its effects, an open row
//! (`Some` tail) is its effects plus whatever the tail variable resolves to,
//! and the empty closed row is the pure row `{}`.
//!
//! Effects are keyed by their constructor head [`Name`], so several effects may
//! share a head (`Tool<ReadRepo>` and `Tool<CreateTicket>` both key under
//! `Tool`). The head is substitution-stable; the type arguments of a
//! [`Effect::Parametric`] are not, so equality and de-duplication compare
//! *resolved* arguments rather than a structural order over unsolved variables.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::fmt::Write as _;

use crate::name::Name;
use crate::ty::Type;

/// A row variable: an index into the row union-find of a [`crate::Subst`].
///
/// A distinct kind from a type variable (the bare `u32` of [`Type::TyVar`]):
/// the newtype makes binding a type variable to a row, or a row variable to a
/// type, a compile error rather than a runtime check.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RowVar(u32);

impl RowVar {
    /// Wraps a raw index as a row variable.
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// The raw index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

impl fmt::Display for RowVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_row_var(f, self.0)
    }
}

/// Renders row variable `id` as `r`, `r1`, `r2`, … — a lowercase form distinct
/// from the `a, b, c` of type variables, so the two kinds read apart. The
/// mapping is a bijection, so distinct variables never collide.
pub(crate) fn write_row_var(f: &mut fmt::Formatter<'_>, id: u32) -> fmt::Result {
    f.write_str("r")?;
    if id > 0 {
        write!(f, "{id}")?;
    }
    Ok(())
}

/// A single effect.
///
/// Either a nullary effect (`Log`) or one applied to type arguments
/// (`Tool<ReadRepo>`, `EtsRead<Table<K, V, Read>>`). The arguments are types so
/// that capability-linked effects can reference a specific value's type.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Effect {
    /// A nullary effect, e.g. `Log`.
    Named(Name),
    /// An effect applied to type arguments, e.g. `Tool<ReadRepo>`.
    Parametric(Name, Vec<Type>),
}

impl Effect {
    /// A nullary effect.
    #[must_use]
    pub fn named(name: impl Into<Name>) -> Self {
        Self::Named(name.into())
    }

    /// An effect applied to type arguments.
    #[must_use]
    pub fn parametric(name: impl Into<Name>, args: Vec<Type>) -> Self {
        Self::Parametric(name.into(), args)
    }

    /// The constructor head, the substitution-stable key an effect is filed
    /// under.
    #[must_use]
    pub fn head(&self) -> &Name {
        match self {
            Self::Named(name) | Self::Parametric(name, _) => name,
        }
    }

    /// The type arguments; empty for a [`Effect::Named`] effect.
    #[must_use]
    pub fn args(&self) -> &[Type] {
        match self {
            Self::Named(_) => &[],
            Self::Parametric(_, args) => args,
        }
    }

    /// Maps `f` over the effect's type arguments, preserving the head. A
    /// [`Effect::Named`] effect has no arguments and is returned unchanged.
    #[must_use]
    pub fn map_args(&self, f: impl FnMut(&Type) -> Type) -> Self {
        match self {
            Self::Named(name) => Self::Named(name.clone()),
            Self::Parametric(name, args) => {
                Self::Parametric(name.clone(), args.iter().map(f).collect())
            }
        }
    }
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => fmt::Display::fmt(name, f),
            Self::Parametric(name, args) => {
                fmt::Display::fmt(name, f)?;
                if let [first, rest @ ..] = args.as_slice() {
                    f.write_str("<")?;
                    fmt::Display::fmt(first, f)?;
                    for arg in rest {
                        f.write_str(", ")?;
                        fmt::Display::fmt(arg, f)?;
                    }
                    f.write_str(">")?;
                }
                Ok(())
            }
        }
    }
}

/// An effect row: an idempotent set of effects with an optional tail variable.
///
/// The tail encodes the closed/open/empty distinction: `None` is a closed row
/// (exactly its effects), `Some` is an open row (its effects plus the tail),
/// and an empty map with `None` is the pure row `{}`. Effects are held keyed by
/// head [`Name`], with several effects allowed per head.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct EffectRow {
    /// Effects keyed by constructor head; each bucket holds the effects sharing
    /// that head, in insertion order.
    effects: BTreeMap<Name, Vec<Effect>>,
    /// The row tail: `None` closed, `Some` open.
    tail: Option<RowVar>,
}

impl EffectRow {
    /// The empty (pure) closed row `{}`.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// A closed row of the given effects.
    #[must_use]
    pub fn closed(effects: impl IntoIterator<Item = Effect>) -> Self {
        let mut row = Self::empty();
        for effect in effects {
            row.insert(effect);
        }
        row
    }

    /// An open row: the given effects extended by `tail`.
    #[must_use]
    pub fn open(effects: impl IntoIterator<Item = Effect>, tail: RowVar) -> Self {
        let mut row = Self::closed(effects);
        row.tail = Some(tail);
        row
    }

    /// The bare open row `{tail}` — no concrete effects, just a row variable.
    #[must_use]
    pub fn of_var(tail: RowVar) -> Self {
        Self {
            effects: BTreeMap::new(),
            tail: Some(tail),
        }
    }

    /// Adds `effect`, keeping set semantics: an effect structurally equal to one
    /// already present under the same head is dropped.
    pub fn insert(&mut self, effect: Effect) {
        let bucket = self.effects.entry(effect.head().clone()).or_default();
        if !bucket.contains(&effect) {
            bucket.push(effect);
        }
    }

    /// Replaces the tail, returning `self` for chaining.
    #[must_use]
    pub fn with_tail(mut self, tail: Option<RowVar>) -> Self {
        self.tail = tail;
        self
    }

    /// The tail row variable, if the row is open.
    #[must_use]
    pub fn tail(&self) -> Option<RowVar> {
        self.tail
    }

    /// Whether the row is the pure row `{}`: no effects and a closed tail.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty() && self.tail.is_none()
    }

    /// Every effect, in head order then insertion order.
    pub fn effects(&self) -> impl Iterator<Item = &Effect> + '_ {
        self.effects.values().flatten()
    }

    /// The effects keyed by head, for crate-internal unification and resolution.
    pub(crate) fn buckets(&self) -> &BTreeMap<Name, Vec<Effect>> {
        &self.effects
    }

    /// Sets the tail in place (crate-internal rebuilding).
    pub(crate) fn set_tail(&mut self, tail: Option<RowVar>) {
        self.tail = tail;
    }

    /// Sorts each head's bucket by rendered form, giving a canonical within-head
    /// order so two semantically-equal resolved rows compare equal regardless of
    /// the order their effects were inserted. Applied after resolution, when
    /// arguments are concrete and renderings stable.
    pub(crate) fn sort_buckets(&mut self) {
        for bucket in self.effects.values_mut() {
            if bucket.len() < 2 {
                continue;
            }
            bucket.sort_by_cached_key(|effect| {
                let mut key = String::new();
                let _ = write!(key, "{effect}");
                key
            });
        }
    }
}

impl fmt::Display for EffectRow {
    /// Renders `{}`, `{r}`, `{Log}`, `{Log, Tool<X>}`, or `{Log, Tool<X> | r}`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{")?;
        let mut first = true;
        for effect in self.effects() {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            fmt::Display::fmt(effect, f)?;
        }
        if let Some(tail) = self.tail {
            // `{r}` has no leading separator; `{Log | r}` does.
            if first {
                fmt::Display::fmt(&tail, f)?;
            } else {
                f.write_str(" | ")?;
                fmt::Display::fmt(&tail, f)?;
            }
        }
        f.write_str("}")
    }
}

/// The effect row of a DI-style `handle` block: the body's effects with the
/// handled effects removed, then the handlers' own effects added — `(body −
/// handled) ∪ handler`.
///
/// All three rows must be resolved (their effect arguments substituted), so
/// effects compare by concrete head and arguments rather than by unsolved
/// variable identity. An effect of `body` is dropped iff it equals a `handled`
/// effect; the body's open tail — its unhandled, unknown remainder — is
/// preserved, and an open handler row keeps the result open too.
#[must_use]
pub fn handle_row(body: &EffectRow, handled: &EffectRow, handler: &EffectRow) -> EffectRow {
    let mut out = EffectRow::empty();
    for effect in body.effects() {
        if !handled.effects().any(|present| present == effect) {
            out.insert(effect.clone());
        }
    }
    for effect in handler.effects() {
        out.insert(effect.clone());
    }
    out.with_tail(body.tail().or_else(|| handler.tail()))
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::vec;

    use super::{Effect, EffectRow, RowVar, handle_row};
    use crate::ty::Type;

    #[test]
    fn empty_row_renders_braces() {
        assert_eq!(format!("{}", EffectRow::empty()), "{}");
    }

    #[test]
    fn single_named_effect() {
        let row = EffectRow::closed([Effect::named("Log")]);
        assert_eq!(format!("{row}"), "{Log}");
    }

    #[test]
    fn named_and_parametric_render_head_sorted() {
        // Inserted Tool-first; head order (BTreeMap) puts Log before Tool.
        let row = EffectRow::closed([
            Effect::parametric("Tool", vec![Type::con("X", vec![])]),
            Effect::named("Log"),
        ]);
        assert_eq!(format!("{row}"), "{Log, Tool<X>}");
    }

    #[test]
    fn open_row_with_one_effect() {
        let row = EffectRow::open([Effect::named("Log")], RowVar::new(0));
        assert_eq!(format!("{row}"), "{Log | r}");
    }

    #[test]
    fn bare_row_variable() {
        assert_eq!(format!("{}", EffectRow::of_var(RowVar::new(0))), "{r}");
    }

    #[test]
    fn row_variable_suffixes() {
        assert_eq!(format!("{}", RowVar::new(0)), "r");
        assert_eq!(format!("{}", RowVar::new(1)), "r1");
        assert_eq!(format!("{}", RowVar::new(7)), "r7");
    }

    #[test]
    fn insert_is_idempotent_for_equal_effects() {
        let mut row = EffectRow::empty();
        row.insert(Effect::named("Log"));
        row.insert(Effect::named("Log"));
        assert_eq!(row.effects().count(), 1);
    }

    #[test]
    fn several_effects_share_a_head() {
        let row = EffectRow::closed([
            Effect::parametric("Tool", vec![Type::con("ReadRepo", vec![])]),
            Effect::parametric("Tool", vec![Type::con("CreateTicket", vec![])]),
        ]);
        assert_eq!(row.effects().count(), 2);
        assert_eq!(format!("{row}"), "{Tool<ReadRepo>, Tool<CreateTicket>}");
    }

    #[test]
    fn handle_row_subtracts_handled_effect() {
        let body = EffectRow::closed([Effect::named("Log")]);
        let handled = EffectRow::closed([Effect::named("Log")]);
        let row = handle_row(&body, &handled, &EffectRow::empty());
        assert_eq!(format!("{row}"), "{}");
    }

    #[test]
    fn handle_row_leaves_unhandled_effect() {
        let body = EffectRow::closed([
            Effect::named("Log"),
            Effect::parametric("Tool", vec![Type::con("Repo", vec![])]),
        ]);
        let handled = EffectRow::closed([Effect::named("Log")]);
        let row = handle_row(&body, &handled, &EffectRow::empty());
        assert_eq!(format!("{row}"), "{Tool<Repo>}");
    }

    #[test]
    fn handle_row_adds_handler_effects() {
        let tool = Effect::parametric("Tool", vec![Type::con("Repo", vec![])]);
        let body = EffectRow::closed([tool.clone()]);
        let handled = EffectRow::closed([tool]);
        let handler = EffectRow::closed([Effect::named("Log")]);
        let row = handle_row(&body, &handled, &handler);
        assert_eq!(format!("{row}"), "{Log}");
    }

    #[test]
    fn handle_row_partially_handles_same_head() {
        let read = Effect::parametric("Tool", vec![Type::con("ReadRepo", vec![])]);
        let write = Effect::parametric("Tool", vec![Type::con("CreateTicket", vec![])]);
        let body = EffectRow::closed([read.clone(), write]);
        let handled = EffectRow::closed([read]);
        let row = handle_row(&body, &handled, &EffectRow::empty());
        assert_eq!(format!("{row}"), "{Tool<CreateTicket>}");
    }

    #[test]
    fn handle_row_keeps_open_body_tail() {
        let body = EffectRow::open([Effect::named("Log")], RowVar::new(0));
        let handled = EffectRow::closed([Effect::named("Log")]);
        let row = handle_row(&body, &handled, &EffectRow::empty());
        assert_eq!(format!("{row}"), "{r}");
    }
}

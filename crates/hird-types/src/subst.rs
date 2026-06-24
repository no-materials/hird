// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Union-find substitution table backing unification and generalisation.
//!
//! Variables are indices into a slot vector. Each slot is one of three states:
//! an unbound representative carrying a union-by-rank count and a binding
//! level, a link to another variable (the union-find edges), or a solved
//! binding to a type. `find` applies path compression, so lookups amortise to
//! near-constant time.
//!
//! Levels implement Rémy-style generalisation: each fresh variable records
//! the level it was created at, binding lowers the levels of the bound type's
//! free variables (a variable reachable from an outer scope can never be
//! quantified), and [`Subst::generalize`] quantifies exactly the variables
//! whose level is deeper than the current one.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use hird_lex::Span;

use crate::effect::{Effect, EffectRow, RowVar};
use crate::error::TypeError;
use crate::ty::Type;

/// State of a single type variable.
#[derive(Debug)]
enum Slot {
    /// A representative not yet equated to anything.
    Unbound {
        /// Union-by-rank height bound.
        rank: u32,
        /// Level the variable is owned by; deeper than current means
        /// generalisable.
        level: u32,
    },
    /// A union-find edge to another variable in the same class.
    Link(u32),
    /// A solved binding to a non-variable type.
    Solved(Type),
}

/// State of a single row variable: the row-space mirror of [`Slot`].
#[derive(Debug)]
enum RowSlot {
    /// A representative not yet equated to anything.
    Unbound {
        /// Union-by-rank height bound.
        rank: u32,
        /// Level the variable is owned by; deeper than current means
        /// generalisable.
        level: u32,
    },
    /// A union-find edge to another row variable in the same class.
    Link(u32),
    /// A solved binding to an effect row.
    Solved(EffectRow),
}

/// Substitution mapping type variables to types and row variables to effect
/// rows, with union-find sharing and level-tracked generalisation.
///
/// Type variables and row variables are separate union-finds — distinct slot
/// vectors indexed by distinct newtypes — so a type variable can never be bound
/// to a row, nor a row variable to a type (the kind separation is a compile-time
/// property, not a runtime check). They share the single binding [`level`] so
/// generalisation quantifies both kinds against one scope counter.
///
/// [`level`]: Subst::level
#[derive(Debug)]
pub struct Subst {
    /// One slot per allocated type variable, indexed by variable id.
    slots: Vec<Slot>,
    /// One slot per allocated row variable, indexed by [`RowVar`] id.
    row_slots: Vec<RowSlot>,
    /// Current binding level; incremented for the extent of each
    /// generalisation scope (a `let` value or a top-level binding group).
    /// Shared by both variable kinds.
    level: u32,
}

impl Subst {
    /// An empty table with no variables allocated, at the outermost level.
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            row_slots: Vec::new(),
            level: 0,
        }
    }

    /// Allocates a fresh unbound variable at the current level and returns
    /// its id.
    pub fn fresh(&mut self) -> u32 {
        let id = self.slots.len();
        self.slots.push(Slot::Unbound {
            rank: 0,
            level: self.level,
        });
        u32::try_from(id).expect("type-variable count exceeds u32::MAX")
    }

    /// Allocates a fresh variable and returns it as a [`Type`].
    pub fn fresh_type(&mut self) -> Type {
        Type::TyVar(self.fresh())
    }

    /// Allocates a fresh unbound row variable at the current level.
    pub fn fresh_row(&mut self) -> RowVar {
        let id = self.row_slots.len();
        self.row_slots.push(RowSlot::Unbound {
            rank: 0,
            level: self.level,
        });
        RowVar::new(u32::try_from(id).expect("row-variable count exceeds u32::MAX"))
    }

    /// Opens a generalisation scope: variables allocated until the matching
    /// [`Subst::exit_level`] are candidates for quantification.
    pub fn enter_level(&mut self) {
        self.level += 1;
    }

    /// Closes the innermost generalisation scope opened by
    /// [`Subst::enter_level`].
    pub fn exit_level(&mut self) {
        debug_assert!(self.level > 0, "exit_level without matching enter_level");
        self.level -= 1;
    }

    /// Representative of `var`'s class, with path compression.
    fn find(&mut self, var: u32) -> u32 {
        let root = self.find_root(var);
        let mut cur = var;
        while let Slot::Link(n) = &self.slots[cur as usize] {
            // Copy the next link out before mutating, ending the borrow.
            let next = *n;
            self.slots[cur as usize] = Slot::Link(root);
            cur = next;
        }
        root
    }

    /// Representative of `var`'s class without mutation.
    fn find_root(&self, mut var: u32) -> u32 {
        while let Slot::Link(next) = &self.slots[var as usize] {
            var = *next;
        }
        var
    }

    /// Rank stored at a representative; `0` for any non-representative slot.
    fn root_rank(&self, root: u32) -> u32 {
        match &self.slots[root as usize] {
            Slot::Unbound { rank, .. } => *rank,
            _ => 0,
        }
    }

    /// Level stored at a representative; the current level for any
    /// non-representative slot.
    fn root_level(&self, root: u32) -> u32 {
        match &self.slots[root as usize] {
            Slot::Unbound { level, .. } => *level,
            _ => self.level,
        }
    }

    /// Merges the classes of `a` and `b`, linking the shorter tree under the
    /// taller one. The merged class keeps the shallower (outer) of the two
    /// levels. Both must be unbound; callers ensure this before unifying two
    /// variables.
    pub(crate) fn union(&mut self, a: u32, b: u32) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        let level = self.root_level(ra).min(self.root_level(rb));
        let rank_a = self.root_rank(ra);
        let rank_b = self.root_rank(rb);
        let (child, root, rank) = if rank_a < rank_b {
            (ra, rb, rank_b)
        } else if rank_a > rank_b {
            (rb, ra, rank_a)
        } else {
            (rb, ra, rank_a + 1)
        };
        self.slots[child as usize] = Slot::Link(root);
        self.slots[root as usize] = Slot::Unbound { rank, level };
    }

    // ── row union-find ──────────────────────────────────────────────
    //
    // The row-space mirror of the type-variable union-find above: the same
    // find/union/bind machinery over [`RowSlot`], indexed by [`RowVar`]. Kept
    // a separate set of methods (rather than generalised) so the two kinds
    // cannot be confused at a call site.

    /// Representative of row variable `var`'s class, with path compression.
    fn row_find(&mut self, var: RowVar) -> RowVar {
        let root = self.row_find_root(var);
        let mut cur = var.index();
        while let RowSlot::Link(n) = &self.row_slots[cur as usize] {
            let next = *n;
            self.row_slots[cur as usize] = RowSlot::Link(root.index());
            cur = next;
        }
        root
    }

    /// Representative of row variable `var`'s class without mutation.
    fn row_find_root(&self, mut var: RowVar) -> RowVar {
        while let RowSlot::Link(next) = &self.row_slots[var.index() as usize] {
            var = RowVar::new(*next);
        }
        var
    }

    /// Rank stored at a row representative; `0` for any non-representative slot.
    fn row_root_rank(&self, root: RowVar) -> u32 {
        match &self.row_slots[root.index() as usize] {
            RowSlot::Unbound { rank, .. } => *rank,
            _ => 0,
        }
    }

    /// Level stored at a row representative; the current level for any
    /// non-representative slot.
    fn row_root_level(&self, root: RowVar) -> u32 {
        match &self.row_slots[root.index() as usize] {
            RowSlot::Unbound { level, .. } => *level,
            _ => self.level,
        }
    }

    /// Merges the classes of row variables `a` and `b`, keeping the shallower
    /// level. Both must be unbound; callers ensure this before equating two row
    /// variables.
    pub(crate) fn row_union(&mut self, a: RowVar, b: RowVar) {
        let ra = self.row_find(a);
        let rb = self.row_find(b);
        if ra == rb {
            return;
        }
        let level = self.row_root_level(ra).min(self.row_root_level(rb));
        let rank_a = self.row_root_rank(ra);
        let rank_b = self.row_root_rank(rb);
        let (child, root, rank) = if rank_a < rank_b {
            (ra, rb, rank_b)
        } else if rank_a > rank_b {
            (rb, ra, rank_a)
        } else {
            (rb, ra, rank_a + 1)
        };
        self.row_slots[child.index() as usize] = RowSlot::Link(root.index());
        self.row_slots[root.index() as usize] = RowSlot::Unbound { rank, level };
    }

    /// Binds row variable `var` to `row`, first checking that `var` does not
    /// occur in `row`'s tail chain (which would describe an infinite row). The
    /// same walk lowers the level of every free variable of `row` — type
    /// variables inside parametric effects and nested tail row variables — to
    /// `var`'s level. To equate two row variables, call [`Subst::row_union`].
    pub(crate) fn row_bind(
        &mut self,
        var: RowVar,
        row: EffectRow,
        span: Span,
    ) -> Result<(), TypeError> {
        let root = self.row_find(var);
        if self.row_tail_contains(root, &row) {
            return Err(TypeError::InfiniteEffectRow {
                var: root,
                in_row: Box::new(self.resolve_row(&row)),
                span,
            });
        }
        let level = self.row_root_level(root);
        self.lower_row(level, &row);
        self.row_slots[root.index() as usize] = RowSlot::Solved(row);
        Ok(())
    }

    /// Whether row representative `target` appears in `row`'s tail chain. Row
    /// variables only ever appear as tails (never inside a type), so the tail
    /// walk is the whole occurs check.
    fn row_tail_contains(&self, target: RowVar, row: &EffectRow) -> bool {
        let mut cur = row.tail();
        while let Some(rv) = cur {
            let root = self.row_find_root(rv);
            if root == target {
                return true;
            }
            match &self.row_slots[root.index() as usize] {
                RowSlot::Solved(solved) => cur = solved.tail(),
                _ => return false,
            }
        }
        false
    }

    /// Binds `var` to `ty`, first checking that `var` does not occur within
    /// `ty` (which would describe an infinite type). The same walk lowers the
    /// level of every free variable of `ty` to `var`'s level, preserving the
    /// invariant that a variable reachable from an outer scope is never
    /// quantified. `ty` must be a non-variable type; to equate two variables,
    /// call [`Subst::union`].
    pub(crate) fn bind(&mut self, var: u32, ty: Type, span: Span) -> Result<(), TypeError> {
        let root = self.find(var);
        let level = self.root_level(root);
        if self.occurs_adjust(root, level, &ty) {
            return Err(TypeError::InfiniteType {
                var: root,
                in_type: Box::new(self.resolve(&ty)),
                span,
            });
        }
        self.slots[root as usize] = Slot::Solved(ty);
        Ok(())
    }

    /// Whether representative `var` appears anywhere in `ty`, following bound
    /// variables through the current substitution. As a side effect, lowers
    /// every reached unbound variable's level to at most `level`.
    fn occurs_adjust(&mut self, var: u32, level: u32, ty: &Type) -> bool {
        match ty {
            Type::TyVar(v) => {
                let root = self.find(*v);
                match &mut self.slots[root as usize] {
                    Slot::Solved(t) => {
                        let t = t.clone();
                        self.occurs_adjust(var, level, &t)
                    }
                    Slot::Unbound { level: l, .. } => {
                        if *l > level {
                            *l = level;
                        }
                        root == var
                    }
                    // `find` returned a representative.
                    Slot::Link(_) => unreachable!("find returned a link"),
                }
            }
            Type::TyCon(_, args) => args.iter().any(|a| self.occurs_adjust(var, level, a)),
            Type::TyFn(params, ret, row) => {
                // Short-circuiting `||` is sound: it only skips work once an
                // occurrence is found, and an occurrence aborts the binding, so
                // the level-lowering it skips never matters.
                params.iter().any(|p| self.occurs_adjust(var, level, p))
                    || self.occurs_adjust(var, level, ret)
                    || self.occurs_in_row(var, level, row)
            }
            Type::TyTuple(elems) => elems.iter().any(|e| self.occurs_adjust(var, level, e)),
            Type::TyRecord(fields) => fields.values().any(|t| self.occurs_adjust(var, level, t)),
            Type::TyForall(_, _, body) => self.occurs_adjust(var, level, body),
        }
    }

    /// Whether type variable `var` occurs in `row` (only ever inside a
    /// parametric effect's type arguments), lowering the levels of every
    /// variable `row` reaches — type variables in effect arguments and tail row
    /// variables — to at most `level`. The type-space half of the soundness
    /// obligation that level-lowering crosses into row-space.
    fn occurs_in_row(&mut self, var: u32, level: u32, row: &EffectRow) -> bool {
        let mut occurs = false;
        // `|=` (no short-circuit) so every argument is visited and lowered.
        for effect in row.effects() {
            for arg in effect.args() {
                occurs |= self.occurs_adjust(var, level, arg);
            }
        }
        self.lower_row_tail(level, row.tail());
        occurs
    }

    /// Lowers to at most `level` the levels of every unbound variable reachable
    /// from `ty`: its type variables, the type arguments of effects in any
    /// function row it contains, and those rows' tail variables.
    fn lower_levels(&mut self, level: u32, ty: &Type) {
        match ty {
            Type::TyVar(v) => {
                let root = self.find(*v);
                match &mut self.slots[root as usize] {
                    Slot::Solved(t) => {
                        let t = t.clone();
                        self.lower_levels(level, &t);
                    }
                    Slot::Unbound { level: l, .. } => {
                        if *l > level {
                            *l = level;
                        }
                    }
                    Slot::Link(_) => unreachable!("find returned a link"),
                }
            }
            Type::TyCon(_, args) => {
                for arg in args {
                    self.lower_levels(level, arg);
                }
            }
            Type::TyFn(params, ret, row) => {
                for param in params {
                    self.lower_levels(level, param);
                }
                self.lower_levels(level, ret);
                self.lower_row(level, row);
            }
            Type::TyTuple(elems) => {
                for elem in elems {
                    self.lower_levels(level, elem);
                }
            }
            Type::TyRecord(fields) => {
                for field in fields.values() {
                    self.lower_levels(level, field);
                }
            }
            Type::TyForall(_, _, body) => self.lower_levels(level, body),
        }
    }

    /// Lowers to at most `level` the levels of every variable `row` reaches:
    /// type variables in effect arguments and the tail row variables.
    fn lower_row(&mut self, level: u32, row: &EffectRow) {
        for effect in row.effects() {
            for arg in effect.args() {
                self.lower_levels(level, arg);
            }
        }
        self.lower_row_tail(level, row.tail());
    }

    /// Walks the tail chain from `tail`, lowering each unbound row variable's
    /// level and descending into solved rows' effect arguments.
    fn lower_row_tail(&mut self, level: u32, tail: Option<RowVar>) {
        let mut cur = tail;
        while let Some(rv) = cur {
            let root = self.row_find(rv);
            let solved = match &self.row_slots[root.index() as usize] {
                RowSlot::Solved(solved) => Some(solved.clone()),
                _ => None,
            };
            match solved {
                Some(solved) => {
                    for effect in solved.effects() {
                        for arg in effect.args() {
                            self.lower_levels(level, arg);
                        }
                    }
                    cur = solved.tail();
                }
                None => {
                    if let RowSlot::Unbound { level: l, .. } =
                        &mut self.row_slots[root.index() as usize]
                        && *l > level
                    {
                        *l = level;
                    }
                    cur = None;
                }
            }
        }
    }

    /// Resolves `ty`'s outermost layer: a solved variable yields its binding,
    /// an unbound variable yields its representative, and any non-variable type
    /// is borrowed back unchanged. Only the variable-following cases allocate;
    /// concrete inputs are returned without cloning. Sub-terms are left
    /// unresolved.
    pub(crate) fn head<'a>(&mut self, ty: &'a Type) -> Cow<'a, Type> {
        match ty {
            Type::TyVar(v) => {
                let root = self.find(*v);
                match &self.slots[root as usize] {
                    Slot::Solved(t) => Cow::Owned(t.clone()),
                    _ => Cow::Owned(Type::TyVar(root)),
                }
            }
            other => Cow::Borrowed(other),
        }
    }

    /// Deeply substitutes every variable in `ty`, producing a type whose only
    /// remaining variables are unbound representatives.
    #[must_use]
    pub fn resolve(&self, ty: &Type) -> Type {
        match ty {
            Type::TyVar(v) => {
                let root = self.find_root(*v);
                match &self.slots[root as usize] {
                    Slot::Solved(t) => self.resolve(t),
                    _ => Type::TyVar(root),
                }
            }
            Type::TyCon(name, args) => {
                Type::TyCon(name.clone(), args.iter().map(|a| self.resolve(a)).collect())
            }
            Type::TyFn(params, ret, row) => Type::TyFn(
                params.iter().map(|p| self.resolve(p)).collect(),
                Box::new(self.resolve(ret)),
                self.resolve_row(row),
            ),
            Type::TyTuple(elems) => Type::TyTuple(elems.iter().map(|e| self.resolve(e)).collect()),
            Type::TyRecord(fields) => Type::TyRecord(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), self.resolve(v)))
                    .collect(),
            ),
            Type::TyForall(tvars, rvars, body) => {
                Type::TyForall(tvars.clone(), rvars.clone(), Box::new(self.resolve(body)))
            }
        }
    }

    /// Deeply resolves an effect row into canonical form: every effect's type
    /// arguments are resolved, the open tail is followed through the row
    /// union-find and any solved rows along it are spliced in, effects are
    /// de-duplicated by their resolved form, and the result's only remaining
    /// tail is `None` or an unbound representative.
    #[must_use]
    pub fn resolve_row(&self, row: &EffectRow) -> EffectRow {
        let mut acc = EffectRow::empty();
        for effect in row.effects() {
            acc.insert(self.resolve_effect(effect));
        }
        // Follow the tail chain, splicing solved rows in and stopping at the
        // first unbound representative (or the closed end). The occurs check on
        // binding keeps this chain finite.
        let mut cur = row.tail();
        let mut tail = None;
        while let Some(rv) = cur {
            let root = self.row_find_root(rv);
            match &self.row_slots[root.index() as usize] {
                RowSlot::Solved(solved) => {
                    for effect in solved.effects() {
                        acc.insert(self.resolve_effect(effect));
                    }
                    cur = solved.tail();
                }
                _ => {
                    tail = Some(root);
                    cur = None;
                }
            }
        }
        acc.set_tail(tail);
        acc.sort_buckets();
        acc
    }

    /// Resolves the type arguments of one effect.
    fn resolve_effect(&self, effect: &Effect) -> Effect {
        match effect {
            Effect::Named(name) => Effect::Named(name.clone()),
            Effect::Parametric(name, args) => {
                Effect::Parametric(name.clone(), args.iter().map(|a| self.resolve(a)).collect())
            }
        }
    }

    /// Generalises `ty` into a type scheme: resolves it, then quantifies every
    /// unbound variable whose level is deeper than the current one (in order
    /// of first appearance). Returns the resolved type unchanged when nothing
    /// is quantifiable.
    ///
    /// `ty` must be monomorphic; any [`Type::TyForall`] already inside it is
    /// left untouched and its interior is not collected.
    #[must_use]
    pub fn generalize(&self, ty: &Type) -> Type {
        let resolved = self.resolve(ty);
        let mut tvars = Vec::new();
        let mut rvars = Vec::new();
        self.collect_deep(&resolved, &mut tvars, &mut rvars);
        if tvars.is_empty() && rvars.is_empty() {
            resolved
        } else {
            Type::TyForall(tvars, rvars, Box::new(resolved))
        }
    }

    /// Accumulates the distinct unbound variables of resolved `ty` whose level
    /// is deeper than the current one, in first-appearance order: type
    /// variables into `tvars`, row variables into `rvars`.
    ///
    /// Crucially this descends into function rows — through the type arguments
    /// of parametric effects and to the tail row variable — so a row variable
    /// that should be quantified is not left free (which would let an effect
    /// escape its handler), and vice versa.
    fn collect_deep(&self, ty: &Type, tvars: &mut Vec<u32>, rvars: &mut Vec<RowVar>) {
        match ty {
            Type::TyVar(v) => {
                // `ty` is resolved, so `v` is an unbound representative.
                if self.root_level(*v) > self.level && !tvars.contains(v) {
                    tvars.push(*v);
                }
            }
            Type::TyCon(_, args) => {
                for arg in args {
                    self.collect_deep(arg, tvars, rvars);
                }
            }
            Type::TyFn(params, ret, row) => {
                for param in params {
                    self.collect_deep(param, tvars, rvars);
                }
                self.collect_deep(ret, tvars, rvars);
                self.collect_deep_row(row, tvars, rvars);
            }
            Type::TyTuple(elems) => {
                for elem in elems {
                    self.collect_deep(elem, tvars, rvars);
                }
            }
            Type::TyRecord(fields) => {
                for field in fields.values() {
                    self.collect_deep(field, tvars, rvars);
                }
            }
            // Already-quantified interiors are closed; nothing to collect.
            Type::TyForall(..) => {}
        }
    }

    /// The row half of [`Subst::collect_deep`]: collects deep type variables in
    /// effect arguments and the deep tail row variable. `row` is assumed
    /// resolved, so its tail is `None` or an unbound representative.
    fn collect_deep_row(&self, row: &EffectRow, tvars: &mut Vec<u32>, rvars: &mut Vec<RowVar>) {
        for effect in row.effects() {
            for arg in effect.args() {
                self.collect_deep(arg, tvars, rvars);
            }
        }
        if let Some(rv) = row.tail()
            && self.row_root_level(rv) > self.level
            && !rvars.contains(&rv)
        {
            rvars.push(rv);
        }
    }

    /// Instantiates a type scheme: replaces each type and row variable
    /// quantified by an outermost [`Type::TyForall`] with a fresh variable of
    /// its kind at the current level. A monomorphic type is returned as a plain
    /// clone.
    #[must_use]
    pub fn instantiate(&mut self, ty: &Type) -> Type {
        match ty {
            Type::TyForall(tvars, rvars, body) => {
                let types: BTreeMap<u32, Type> =
                    tvars.iter().map(|v| (*v, self.fresh_type())).collect();
                let rows: BTreeMap<RowVar, RowVar> =
                    rvars.iter().map(|v| (*v, self.fresh_row())).collect();
                body.substitute(&types, &rows)
            }
            other => other.clone(),
        }
    }
}

impl Default for Subst {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use alloc::format;
    use alloc::vec;

    use hird_lex::Span;

    use super::Subst;
    use crate::effect::{Effect, EffectRow};
    use crate::ty::Type;
    use crate::unify::{unify, unify_row};

    // Docstring notation: α, β, γ are unification variables; a superscript
    // is the level a variable was created at (`α¹` was born at level 1).
    // `~` is "unify", `⇒` the outcome, `{α ↦ T}` a recorded solution.
    // `genₙ` generalises with the current level back at n; `inst`
    // instantiates a scheme with fresh variables.

    /// A throwaway span; these tests never inspect span contents.
    fn span() -> Span {
        Span::new(0, 0, 0)
    }

    // -- generalisation ------------------------------------------------------

    /// `gen₀(α¹ → α¹) = ∀α. α → α` — born deeper than the current level
    /// ⇒ quantified.
    #[test]
    fn generalize_quantifies_deeper_variables() {
        let mut s = Subst::new();
        s.enter_level();
        let a = s.fresh();
        let ty = Type::func(vec![Type::var(a)], Type::var(a));
        s.exit_level();
        let scheme = s.generalize(&ty);
        assert_eq!(
            format!("{}", scheme.normalized()),
            "\u{2200}a. a \u{2192} a"
        );
    }

    /// `gen₀(α⁰ → α⁰) = α⁰ → α⁰` — born at the current level, not deeper
    /// ⇒ stays monomorphic.
    #[test]
    fn generalize_skips_current_level_variables() {
        let mut s = Subst::new();
        let a = s.fresh();
        let ty = Type::func(vec![Type::var(a)], Type::var(a));
        assert_eq!(s.generalize(&ty), s.resolve(&ty));
    }

    /// `{α¹ ↦ Int} ⇒ gen₀(α) = Int` — solved variables disappear into
    /// their solutions before quantification.
    #[test]
    fn generalize_resolves_solved_variables() {
        let mut s = Subst::new();
        s.enter_level();
        let a = s.fresh();
        unify(&mut s, &Type::var(a), &Type::int(), span()).unwrap();
        s.exit_level();
        assert_eq!(s.generalize(&Type::var(a)), Type::int());
    }

    /// `α⁰ ~ List<β¹> ⇒ β lowered to level 0, so gen₀(β) = β` — a variable
    /// that escapes into an outer scope must stay monomorphic.
    #[test]
    fn binding_lowers_levels_to_block_escaping_variables() {
        let mut s = Subst::new();
        let outer = s.fresh();
        s.enter_level();
        let inner = s.fresh();
        unify(
            &mut s,
            &Type::var(outer),
            &Type::list(Type::var(inner)),
            span(),
        )
        .unwrap();
        s.exit_level();
        let scheme = s.generalize(&Type::var(inner));
        assert!(
            !matches!(scheme, Type::TyForall(..)),
            "escaped variable must stay monomorphic, got {scheme:?}"
        );
    }

    /// `β¹ ~ α⁰ ⇒ class {α, β} at level 0, so gen₀(β) = β` — a merged
    /// class keeps the shallower level.
    #[test]
    fn union_keeps_the_outer_level() {
        let mut s = Subst::new();
        let outer = s.fresh();
        s.enter_level();
        let inner = s.fresh();
        unify(&mut s, &Type::var(inner), &Type::var(outer), span()).unwrap();
        s.exit_level();
        let scheme = s.generalize(&Type::var(inner));
        assert!(
            !matches!(scheme, Type::TyForall(..)),
            "variable united with an outer one must stay monomorphic, got {scheme:?}"
        );
    }

    // -- instantiation -------------------------------------------------------

    /// `inst(∀α. α → α) = β → β; inst again = γ → γ, β ≠ γ` — the two
    /// copies then solve independently (`β ↦ Int`, `γ ↦ String`).
    #[test]
    fn instantiate_replaces_quantified_variables_freshly() {
        let mut s = Subst::new();
        s.enter_level();
        let a = s.fresh();
        let ty = Type::func(vec![Type::var(a)], Type::var(a));
        s.exit_level();
        let scheme = s.generalize(&ty);

        let inst1 = s.instantiate(&scheme);
        let inst2 = s.instantiate(&scheme);
        assert_ne!(inst1, inst2, "each instantiation must be fresh");

        // One instantiation can solve to Int while the other solves to String.
        let Type::TyFn(params, _, _) = &inst1 else {
            panic!("expected a function type, got {inst1:?}");
        };
        unify(&mut s, &params[0], &Type::int(), span()).unwrap();
        let Type::TyFn(params, _, _) = &inst2 else {
            panic!("expected a function type, got {inst2:?}");
        };
        unify(&mut s, &params[0], &Type::string(), span()).unwrap();
    }

    /// `inst(Int → String) = Int → String` — no binder, nothing to refresh.
    #[test]
    fn instantiate_returns_monomorphic_types_unchanged() {
        let mut s = Subst::new();
        let ty = Type::func(vec![Type::int()], Type::string());
        assert_eq!(s.instantiate(&ty), ty);
    }

    /// `inst(∀β. β → α) = γ → α` — only quantified variables refresh; the
    /// free `α` survives by identity.
    #[test]
    fn instantiate_keeps_free_variables_shared() {
        let mut s = Subst::new();
        let free = s.fresh();
        s.enter_level();
        let bound = s.fresh();
        let ty = Type::func(vec![Type::var(bound)], Type::var(free));
        s.exit_level();
        let scheme = s.generalize(&ty);

        let Type::TyFn(_, ret, _) = s.instantiate(&scheme) else {
            panic!("expected a function type");
        };
        assert_eq!(*ret, Type::var(free));
    }

    // -- row generalisation and escape ---------------------------------------

    /// `gen₀(() → Int ! {ρ¹}) = ∀ρ. () → Int ! {ρ}` — a row variable born
    /// deeper than the current level is quantified, just like a type variable.
    #[test]
    fn generalize_quantifies_deeper_row_variable() {
        let mut s = Subst::new();
        s.enter_level();
        let r = s.fresh_row();
        let ty = Type::func_eff(vec![], Type::int(), EffectRow::of_var(r));
        s.exit_level();
        let scheme = s.generalize(&ty);
        assert!(
            matches!(&scheme, Type::TyForall(tvars, rvars, _) if tvars.is_empty() && rvars.len() == 1),
            "the row variable must be quantified, got {scheme:?}"
        );
    }

    /// `ρ¹ ~ ρ⁰ ⇒ gen₀(() → Int ! {ρ¹}) is monomorphic` — a row variable
    /// merged into an outer scope must NOT be quantified, or an effect would
    /// escape its handler. The direct escape test the binding rules demand.
    #[test]
    fn generalize_skips_escaped_row_variable() {
        let mut s = Subst::new();
        let outer = s.fresh_row();
        s.enter_level();
        let inner = s.fresh_row();
        // Constrain the inner row variable to the outer one (level 0).
        unify_row(
            &mut s,
            &EffectRow::of_var(inner),
            &EffectRow::of_var(outer),
            span(),
        )
        .unwrap();
        s.exit_level();
        let ty = Type::func_eff(vec![], Type::int(), EffectRow::of_var(inner));
        let scheme = s.generalize(&ty);
        assert!(
            !matches!(scheme, Type::TyForall(..)),
            "escaped row variable must stay monomorphic, got {scheme:?}"
        );
    }

    /// Each instantiation refreshes a quantified row variable, so two copies
    /// solve independently (`ρ ↦ {Log}` in one leaves the other open).
    #[test]
    fn instantiate_refreshes_row_variables() {
        let mut s = Subst::new();
        s.enter_level();
        let r = s.fresh_row();
        let ty = Type::func_eff(vec![], Type::int(), EffectRow::of_var(r));
        s.exit_level();
        let scheme = s.generalize(&ty);

        let inst1 = s.instantiate(&scheme);
        let inst2 = s.instantiate(&scheme);
        let (Type::TyFn(_, _, row1), Type::TyFn(_, _, row2)) = (&inst1, &inst2) else {
            panic!("expected function types, got {inst1:?} / {inst2:?}");
        };
        // Solving one instantiation's row leaves the other unconstrained.
        unify_row(
            &mut s,
            row1,
            &EffectRow::closed([Effect::named("Log")]),
            span(),
        )
        .unwrap();
        assert_eq!(
            s.resolve_row(row1),
            EffectRow::closed([Effect::named("Log")])
        );
        assert!(
            s.resolve_row(row2).tail().is_some(),
            "the second instantiation's row stays open"
        );
    }

    /// `gen₀(() → Int ! {Tool<α¹>})` quantifies `α` — generalisation descends
    /// into a parametric effect's type arguments (else the variable leaks).
    #[test]
    fn generalize_crosses_into_parametric_effect_arguments() {
        let mut s = Subst::new();
        s.enter_level();
        let a = s.fresh();
        let row = EffectRow::closed([Effect::parametric("Tool", vec![Type::var(a)])]);
        let ty = Type::func_eff(vec![], Type::int(), row);
        s.exit_level();
        let scheme = s.generalize(&ty);
        assert!(
            matches!(&scheme, Type::TyForall(tvars, rvars, _) if tvars.len() == 1 && rvars.is_empty()),
            "the effect's type argument must be quantified, got {scheme:?}"
        );
    }

    /// `α¹ ~ Int` inside a row argument lowers `α` to the outer level, so
    /// `gen₀` leaves it monomorphic — level-lowering crosses into parametric
    /// effect arguments.
    #[test]
    fn generalize_skips_escaped_effect_argument() {
        let mut s = Subst::new();
        let outer = s.fresh();
        s.enter_level();
        let a = s.fresh();
        // Bury `a` in a row argument and tie it to the outer variable.
        let ty = Type::func_eff(
            vec![],
            Type::int(),
            EffectRow::closed([Effect::parametric("Tool", vec![Type::var(a)])]),
        );
        unify(&mut s, &Type::var(a), &Type::var(outer), span()).unwrap();
        s.exit_level();
        let scheme = s.generalize(&ty);
        assert!(
            !matches!(scheme, Type::TyForall(..)),
            "escaped effect argument must stay monomorphic, got {scheme:?}"
        );
    }
}

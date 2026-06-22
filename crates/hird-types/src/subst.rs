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

/// Substitution mapping type variables to types, with union-find sharing and
/// level-tracked generalisation.
#[derive(Debug)]
pub struct Subst {
    /// One slot per allocated variable, indexed by variable id.
    slots: Vec<Slot>,
    /// Current binding level; incremented for the extent of each
    /// generalisation scope (a `let` value or a top-level binding group).
    level: u32,
}

impl Subst {
    /// An empty table with no variables allocated, at the outermost level.
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
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
                in_type: self.resolve(&ty),
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
            Type::TyFn(params, ret) => {
                params.iter().any(|p| self.occurs_adjust(var, level, p))
                    || self.occurs_adjust(var, level, ret)
            }
            Type::TyTuple(elems) => elems.iter().any(|e| self.occurs_adjust(var, level, e)),
            Type::TyRecord(fields) => fields.values().any(|t| self.occurs_adjust(var, level, t)),
            Type::TyForall(_, body) => self.occurs_adjust(var, level, body),
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
            Type::TyFn(params, ret) => Type::TyFn(
                params.iter().map(|p| self.resolve(p)).collect(),
                Box::new(self.resolve(ret)),
            ),
            Type::TyTuple(elems) => Type::TyTuple(elems.iter().map(|e| self.resolve(e)).collect()),
            Type::TyRecord(fields) => Type::TyRecord(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), self.resolve(v)))
                    .collect(),
            ),
            Type::TyForall(vars, body) => {
                Type::TyForall(vars.clone(), Box::new(self.resolve(body)))
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
        let mut vars = Vec::new();
        self.collect_deep(&resolved, &mut vars);
        if vars.is_empty() {
            resolved
        } else {
            Type::TyForall(vars, Box::new(resolved))
        }
    }

    /// Accumulates into `vars` the distinct unbound variables of resolved
    /// `ty` whose level is deeper than the current one, in first-appearance
    /// order.
    fn collect_deep(&self, ty: &Type, vars: &mut Vec<u32>) {
        match ty {
            Type::TyVar(v) => {
                // `ty` is resolved, so `v` is an unbound representative.
                if self.root_level(*v) > self.level && !vars.contains(v) {
                    vars.push(*v);
                }
            }
            Type::TyCon(_, args) => {
                for arg in args {
                    self.collect_deep(arg, vars);
                }
            }
            Type::TyFn(params, ret) => {
                for param in params {
                    self.collect_deep(param, vars);
                }
                self.collect_deep(ret, vars);
            }
            Type::TyTuple(elems) => {
                for elem in elems {
                    self.collect_deep(elem, vars);
                }
            }
            Type::TyRecord(fields) => {
                for field in fields.values() {
                    self.collect_deep(field, vars);
                }
            }
            // Already-quantified interiors are closed; nothing to collect.
            Type::TyForall(..) => {}
        }
    }

    /// Instantiates a type scheme: replaces each variable quantified by an
    /// outermost [`Type::TyForall`] with a fresh variable at the current
    /// level. A monomorphic type is returned as a plain clone.
    #[must_use]
    pub fn instantiate(&mut self, ty: &Type) -> Type {
        match ty {
            Type::TyForall(vars, body) => {
                let map: BTreeMap<u32, Type> =
                    vars.iter().map(|v| (*v, self.fresh_type())).collect();
                body.substitute(&map)
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
    use crate::ty::Type;
    use crate::unify::unify;

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
        let Type::TyFn(params, _) = &inst1 else {
            panic!("expected a function type, got {inst1:?}");
        };
        unify(&mut s, &params[0], &Type::int(), span()).unwrap();
        let Type::TyFn(params, _) = &inst2 else {
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

        let Type::TyFn(_, ret) = s.instantiate(&scheme) else {
            panic!("expected a function type");
        };
        assert_eq!(*ret, Type::var(free));
    }
}

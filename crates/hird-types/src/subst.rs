// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Union-find substitution table backing unification.
//!
//! Variables are indices into a slot vector. Each slot is either an unbound
//! representative carrying a union-by-rank count, or bound to a type (a chain
//! of `Bound(TyVar(_))` links forms the union structure). `find` applies path
//! compression, so lookups amortise to near-constant time.

use alloc::boxed::Box;
use alloc::vec::Vec;

use hird_lex::Span;

use crate::error::TypeError;
use crate::ty::Type;

/// State of a single type variable.
#[derive(Debug)]
enum Slot {
    /// A representative not yet equated to anything; `rank` bounds its tree height.
    Unbound {
        /// Union-by-rank height bound.
        rank: u32,
    },
    /// Equated to a type. A `Bound(TyVar(_))` is a union link; any other type
    /// is a solved binding.
    Bound(Type),
}

/// Substitution mapping type variables to types, with union-find sharing.
#[derive(Debug)]
pub struct Subst {
    /// One slot per allocated variable, indexed by variable id.
    slots: Vec<Slot>,
}

impl Subst {
    /// An empty table with no variables allocated.
    #[must_use]
    pub fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// Allocates a fresh unbound variable and returns its id.
    pub fn fresh(&mut self) -> u32 {
        let id = self.slots.len();
        self.slots.push(Slot::Unbound { rank: 0 });
        u32::try_from(id).expect("type-variable count exceeds u32::MAX")
    }

    /// Allocates a fresh variable and returns it as a [`Type`].
    pub fn fresh_type(&mut self) -> Type {
        Type::TyVar(self.fresh())
    }

    /// Representative of `var`'s class, with path compression.
    fn find(&mut self, var: u32) -> u32 {
        let root = self.find_root(var);
        let mut cur = var;
        while let Slot::Bound(Type::TyVar(n)) = &self.slots[cur as usize] {
            // Copy the next link out before mutating, ending the borrow.
            let next = *n;
            self.slots[cur as usize] = Slot::Bound(Type::TyVar(root));
            cur = next;
        }
        root
    }

    /// Representative of `var`'s class without mutation.
    fn find_root(&self, mut var: u32) -> u32 {
        while let Slot::Bound(Type::TyVar(next)) = &self.slots[var as usize] {
            var = *next;
        }
        var
    }

    /// Rank stored at a representative; `0` for any non-representative slot.
    fn root_rank(&self, root: u32) -> u32 {
        match &self.slots[root as usize] {
            Slot::Unbound { rank } => *rank,
            Slot::Bound(_) => 0,
        }
    }

    /// Merges the classes of `a` and `b`, linking the shorter tree under the
    /// taller one. Both must be unbound; callers ensure this before unifying
    /// two variables.
    pub(crate) fn union(&mut self, a: u32, b: u32) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        let rank_a = self.root_rank(ra);
        let rank_b = self.root_rank(rb);
        if rank_a < rank_b {
            self.slots[ra as usize] = Slot::Bound(Type::TyVar(rb));
        } else if rank_a > rank_b {
            self.slots[rb as usize] = Slot::Bound(Type::TyVar(ra));
        } else {
            self.slots[rb as usize] = Slot::Bound(Type::TyVar(ra));
            self.slots[ra as usize] = Slot::Unbound { rank: rank_a + 1 };
        }
    }

    /// Binds `var` to `ty`, first checking that `var` does not occur within
    /// `ty` (which would describe an infinite type).
    pub(crate) fn bind(&mut self, var: u32, ty: Type, span: Span) -> Result<(), TypeError> {
        let root = self.find(var);
        if self.occurs(root, &ty) {
            return Err(TypeError::InfiniteType {
                var: root,
                in_type: self.resolve(&ty),
                span,
            });
        }
        self.slots[root as usize] = Slot::Bound(ty);
        Ok(())
    }

    /// Whether representative `var` appears anywhere in `ty`, following bound
    /// variables through the current substitution.
    fn occurs(&self, var: u32, ty: &Type) -> bool {
        match ty {
            Type::TyVar(v) => {
                let root = self.find_root(*v);
                if root == var {
                    return true;
                }
                match &self.slots[root as usize] {
                    Slot::Unbound { .. } => false,
                    Slot::Bound(t) => self.occurs(var, t),
                }
            }
            Type::TyCon(_, args) => args.iter().any(|a| self.occurs(var, a)),
            Type::TyFn(from, to) => self.occurs(var, from) || self.occurs(var, to),
            Type::TyTuple(elems) => elems.iter().any(|e| self.occurs(var, e)),
            Type::TyRecord(fields) => fields.values().any(|t| self.occurs(var, t)),
            Type::TyForall(_, body) => self.occurs(var, body),
        }
    }

    /// Resolves `ty`'s outermost layer: a bound variable yields its binding, an
    /// unbound variable yields its representative, anything else is returned
    /// as-is. Sub-terms are left unresolved.
    pub(crate) fn head(&mut self, ty: &Type) -> Type {
        match ty {
            Type::TyVar(v) => {
                let root = self.find(*v);
                match &self.slots[root as usize] {
                    Slot::Unbound { .. } => Type::TyVar(root),
                    Slot::Bound(t) => t.clone(),
                }
            }
            other => other.clone(),
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
                    Slot::Unbound { .. } => Type::TyVar(root),
                    Slot::Bound(t) => self.resolve(t),
                }
            }
            Type::TyCon(name, args) => {
                Type::TyCon(name.clone(), args.iter().map(|a| self.resolve(a)).collect())
            }
            Type::TyFn(from, to) => {
                Type::TyFn(Box::new(self.resolve(from)), Box::new(self.resolve(to)))
            }
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
}

impl Default for Subst {
    fn default() -> Self {
        Self::new()
    }
}

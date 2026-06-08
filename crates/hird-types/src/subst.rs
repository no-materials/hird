// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Union-find substitution table backing unification.
//!
//! Variables are indices into a slot vector. Each slot is one of three states:
//! an unbound representative carrying a union-by-rank count, a link to another
//! variable (the union-find edges), or a solved binding to a type. `find`
//! applies path compression, so lookups amortise to near-constant time.

use alloc::borrow::Cow;
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
    /// A union-find edge to another variable in the same class.
    Link(u32),
    /// A solved binding to a non-variable type.
    Solved(Type),
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
            Slot::Unbound { rank } => *rank,
            _ => 0,
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
            self.slots[ra as usize] = Slot::Link(rb);
        } else if rank_a > rank_b {
            self.slots[rb as usize] = Slot::Link(ra);
        } else {
            self.slots[rb as usize] = Slot::Link(ra);
            self.slots[ra as usize] = Slot::Unbound { rank: rank_a + 1 };
        }
    }

    /// Binds `var` to `ty`, first checking that `var` does not occur within
    /// `ty` (which would describe an infinite type). `ty` must be a
    /// non-variable type; to equate two variables, call [`Subst::union`].
    pub(crate) fn bind(&mut self, var: u32, ty: Type, span: Span) -> Result<(), TypeError> {
        let root = self.find(var);
        if self.occurs(root, &ty) {
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
    /// variables through the current substitution.
    fn occurs(&self, var: u32, ty: &Type) -> bool {
        match ty {
            Type::TyVar(v) => {
                let root = self.find_root(*v);
                if root == var {
                    return true;
                }
                match &self.slots[root as usize] {
                    Slot::Solved(t) => self.occurs(var, t),
                    _ => false,
                }
            }
            Type::TyCon(_, args) => args.iter().any(|a| self.occurs(var, a)),
            Type::TyFn(from, to) => self.occurs(var, from) || self.occurs(var, to),
            Type::TyTuple(elems) => elems.iter().any(|e| self.occurs(var, e)),
            Type::TyRecord(fields) => fields.values().any(|t| self.occurs(var, t)),
            Type::TyForall(_, body) => self.occurs(var, body),
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

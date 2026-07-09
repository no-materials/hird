// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The ADT registry: declared types, their constructors, and built-ins.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use hird_types::{Name, Type};

use crate::ModuleName;

/// A declared algebraic data type.
#[derive(Debug)]
pub(crate) struct AdtInfo {
    /// Number of type parameters.
    pub(crate) arity: usize,
    /// Constructor names in declaration order.
    pub(crate) constructors: Vec<Name>,
}

/// A declared data constructor.
#[derive(Debug)]
pub(crate) struct CtorInfo {
    /// Scheme of the constructor as a value: `∀params. fields → Adt<params>`
    /// for constructors with fields, or the bare instance type when nullary.
    pub(crate) scheme: Type,
    /// The type this constructs.
    pub(crate) owner: Name,
    /// Module that declares the constructor; `None` for the current module's
    /// own declarations and built-ins. Construction and destructuring outside
    /// this module are gated when the owning type is opaque.
    pub(crate) module: Option<ModuleName>,
    /// Whether the owning type is opaque (constructors module-private).
    pub(crate) opaque: bool,
}

/// Declared types and constructors, plus built-in type arities.
#[derive(Debug)]
pub(crate) struct Registry {
    /// Declared (and seeded) ADTs by type name.
    adts: BTreeMap<Name, AdtInfo>,
    /// Declared (and seeded) constructors by constructor name.
    ctors: BTreeMap<Name, CtorInfo>,
    /// Declared effects by name, mapped to their type-parameter count.
    effects: BTreeMap<Name, usize>,
}

impl Registry {
    /// A registry holding only the built-in `Bool` ADT.
    ///
    /// `Bool` is predeclared as if `type Bool = True | False` had been
    /// written: its values are the constructors, and exhaustiveness over it
    /// needs no special-casing.
    pub(crate) fn new() -> Self {
        let mut registry = Self {
            adts: BTreeMap::new(),
            ctors: BTreeMap::new(),
            effects: BTreeMap::new(),
        };
        registry.declare_adt(
            Name::new("Bool"),
            0,
            Vec::from([Name::new("True"), Name::new("False")]),
        );
        registry.declare_ctor(
            Name::new("True"),
            CtorInfo {
                scheme: Type::bool(),
                owner: Name::new("Bool"),
                module: None,
                opaque: false,
            },
        );
        registry.declare_ctor(
            Name::new("False"),
            CtorInfo {
                scheme: Type::bool(),
                owner: Name::new("Bool"),
                module: None,
                opaque: false,
            },
        );
        registry
    }

    /// Registers a type declaration's header. Later declarations sharing a
    /// name replace earlier ones; duplicate detection is a module-system
    /// concern.
    pub(crate) fn declare_adt(&mut self, name: Name, arity: usize, constructors: Vec<Name>) {
        self.adts.insert(
            name,
            AdtInfo {
                arity,
                constructors,
            },
        );
    }

    /// Registers a constructor.
    pub(crate) fn declare_ctor(&mut self, name: Name, info: CtorInfo) {
        self.ctors.insert(name, info);
    }

    /// Registers an effect declaration's name and type-parameter count. A later
    /// declaration sharing a name replaces the earlier one.
    pub(crate) fn declare_effect(&mut self, name: Name, arity: usize) {
        self.effects.insert(name, arity);
    }

    /// The type-parameter count of effect `name`, if it is declared.
    pub(crate) fn effect_arity(&self, name: &str) -> Option<usize> {
        self.effects.get(&Name::new(name)).copied()
    }

    /// The constructor named `name`, if declared.
    pub(crate) fn ctor(&self, name: &str) -> Option<&CtorInfo> {
        self.ctors.get(&Name::new(name))
    }

    /// The arity of the type constructor `name`: declared ADTs first, then
    /// the built-ins (`Int`, `Float`, `String`, `List`, `Option`, and the
    /// actor references `Pid`, `ReplyTo`).
    pub(crate) fn type_arity(&self, name: &str) -> Option<usize> {
        if let Some(info) = self.adts.get(&Name::new(name)) {
            return Some(info.arity);
        }
        match name {
            "Int" | "Float" | "String" => Some(0),
            "List" | "Option" | "Pid" | "ReplyTo" => Some(1),
            _ => None,
        }
    }

    /// Whether `name` is a declared ADT whose constructors are module-private
    /// (an opaque capability type).
    pub(crate) fn adt_is_opaque(&self, name: &str) -> bool {
        self.adts.get(&Name::new(name)).is_some_and(|info| {
            info.constructors
                .iter()
                .any(|ctor| self.ctors.get(ctor).is_some_and(|info| info.opaque))
        })
    }

    /// Declared ADTs with their constructor lists, in name order.
    pub(crate) fn adt_summaries(&self) -> impl Iterator<Item = (&Name, &Vec<Name>)> {
        self.adts
            .iter()
            .map(|(name, info)| (name, &info.constructors))
    }

    /// The constructor names of `name`, if it is a declared (or seeded) ADT.
    ///
    /// Returns `None` for non-ADT type constructors — the built-ins `Int`,
    /// `Float`, `String`, and any `List`/`Option` that no declaration backs.
    /// Their value space is open, so exhaustiveness over them is decided by a
    /// catch-all rather than a constructor enumeration.
    pub(crate) fn adt_constructors(&self, name: &str) -> Option<&[Name]> {
        self.adts
            .get(&Name::new(name))
            .map(|info| info.constructors.as_slice())
    }

    /// Whether constructor `name` carries a `ReplyTo` field directly — a "call
    /// constructor" in the actor protocol, usable only as `request`'s message
    /// builder. Undeclared or nullary constructors carry none.
    pub(crate) fn ctor_carries_reply_to(&self, name: &str) -> bool {
        self.ctor(name)
            .is_some_and(|info| ctor_fields(&info.scheme).iter().any(is_reply_to))
    }

    /// The field types of constructor `name` — the parameter types of its
    /// scheme — or empty when the constructor is nullary or undeclared.
    pub(crate) fn ctor_field_types(&self, name: &str) -> Vec<Type> {
        self.ctor(name)
            .map(|info| ctor_fields(&info.scheme).to_vec())
            .unwrap_or_default()
    }

    /// Whether `ty` mentions `ReplyTo` anywhere, resolving named ADTs through
    /// their declared constructors so a `ReplyTo` baked into a type's fields is
    /// found. Cycle-guarded, so a recursive type terminates.
    pub(crate) fn contains_reply_to(&self, ty: &Type) -> bool {
        self.reply_to_within(ty, &mut BTreeSet::new())
    }

    /// The cycle-guarded walk behind [`Registry::contains_reply_to`]. A named
    /// type is descended once (tracked in `visited`); a type argument's
    /// `ReplyTo` is found directly, so instantiation need not be modelled.
    fn reply_to_within(&self, ty: &Type, visited: &mut BTreeSet<Name>) -> bool {
        match ty {
            Type::TyVar(_) => false,
            Type::TyCon(name, args) => {
                if name.as_str() == "ReplyTo" {
                    return true;
                }
                if args.iter().any(|arg| self.reply_to_within(arg, visited)) {
                    return true;
                }
                match self.adts.get(name) {
                    Some(info) if visited.insert(name.clone()) => {
                        let ctors = info.constructors.clone();
                        ctors.iter().any(|ctor| {
                            self.ctor_field_types(ctor.as_str())
                                .iter()
                                .any(|field| self.reply_to_within(field, visited))
                        })
                    }
                    _ => false,
                }
            }
            Type::TyFn(params, ret, _) => {
                params.iter().any(|p| self.reply_to_within(p, visited))
                    || self.reply_to_within(ret, visited)
            }
            Type::TyTuple(elems) => elems.iter().any(|e| self.reply_to_within(e, visited)),
            Type::TyRecord(fields) => fields.values().any(|v| self.reply_to_within(v, visited)),
            Type::TyForall(_, _, inner) => self.reply_to_within(inner, visited),
        }
    }
}

/// The parameter (field) types of a constructor scheme, or an empty slice when
/// the constructor is nullary. Strips the generalisation quantifier a declared
/// ADT's scheme carries.
fn ctor_fields(scheme: &Type) -> &[Type] {
    let body = match scheme {
        Type::TyForall(_, _, inner) => inner.as_ref(),
        other => other,
    };
    match body {
        Type::TyFn(params, _, _) => params,
        _ => &[],
    }
}

/// Whether `ty`'s head is the built-in `ReplyTo`.
pub(crate) fn is_reply_to(ty: &Type) -> bool {
    matches!(ty, Type::TyCon(name, _) if name.as_str() == "ReplyTo")
}

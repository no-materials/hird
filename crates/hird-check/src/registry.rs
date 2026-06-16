// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The ADT registry: declared types, their constructors, and built-ins.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use hird_types::{Name, Type};

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
}

/// Declared types and constructors, plus built-in type arities.
#[derive(Debug)]
pub(crate) struct Registry {
    /// Declared (and seeded) ADTs by type name.
    adts: BTreeMap<Name, AdtInfo>,
    /// Declared (and seeded) constructors by constructor name.
    ctors: BTreeMap<Name, CtorInfo>,
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
            },
        );
        registry.declare_ctor(
            Name::new("False"),
            CtorInfo {
                scheme: Type::bool(),
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

    /// The constructor named `name`, if declared.
    pub(crate) fn ctor(&self, name: &str) -> Option<&CtorInfo> {
        self.ctors.get(&Name::new(name))
    }

    /// The arity of the type constructor `name`: declared ADTs first, then
    /// the built-ins (`Int`, `Float`, `String`, `List`, `Option`).
    pub(crate) fn type_arity(&self, name: &str) -> Option<usize> {
        if let Some(info) = self.adts.get(&Name::new(name)) {
            return Some(info.arity);
        }
        match name {
            "Int" | "Float" | "String" => Some(0),
            "List" | "Option" => Some(1),
            _ => None,
        }
    }

    /// Declared ADTs with their constructor lists, in name order.
    pub(crate) fn adt_summaries(&self) -> impl Iterator<Item = (&Name, &Vec<Name>)> {
        self.adts
            .iter()
            .map(|(name, info)| (name, &info.constructors))
    }
}

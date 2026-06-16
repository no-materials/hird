// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The value environment: lexically scoped name-to-scheme bindings.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use hird_types::Type;

/// A stack of lexical scopes mapping value names to types or type schemes.
///
/// The root frame holds top-level bindings (functions, externs, and
/// constructors); `let`, lambda parameters, and match-arm patterns each push
/// a frame.
#[derive(Debug)]
pub(crate) struct Env {
    /// Scope frames, innermost last. Never empty: index 0 is the root frame.
    frames: Vec<BTreeMap<String, Type>>,
}

impl Env {
    /// An environment holding only the (empty) root frame.
    pub(crate) fn new() -> Self {
        Self {
            frames: Vec::from([BTreeMap::new()]),
        }
    }

    /// Opens a new innermost scope.
    pub(crate) fn push_scope(&mut self) {
        self.frames.push(BTreeMap::new());
    }

    /// Closes the innermost scope.
    pub(crate) fn pop_scope(&mut self) {
        debug_assert!(self.frames.len() > 1, "pop_scope would drop the root frame");
        self.frames.pop();
    }

    /// The binding for `name`, searching innermost-first.
    pub(crate) fn lookup(&self, name: &str) -> Option<&Type> {
        self.frames.iter().rev().find_map(|f| f.get(name))
    }

    /// Binds `name` in the innermost scope. Returns `true` when an existing
    /// visible binding (in any frame) is shadowed, so the caller can warn.
    pub(crate) fn insert(&mut self, name: &str, ty: Type) -> bool {
        let shadows = self.lookup(name).is_some();
        let frame = self.frames.last_mut().expect("root frame always exists");
        frame.insert(String::from(name), ty);
        shadows
    }

    /// Binds `name` in the root frame regardless of open scopes. Top-level
    /// declarations are not "inner bindings", so no shadow signal is given.
    pub(crate) fn insert_root(&mut self, name: &str, ty: Type) {
        self.frames[0].insert(String::from(name), ty);
    }
}

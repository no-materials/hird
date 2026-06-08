// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Identifiers used inside semantic types.
//!
//! These are the syntax-to-semantics boundary: the parser produces CST
//! tokens, and lowering (a later pass) turns those tokens into owned
//! [`Name`] and [`Label`] values that types can clone freely. They are
//! deliberately plain owned strings rather than interned symbols; an
//! interner is a measured optimisation deferred until profiling demands it.

use alloc::boxed::Box;
use alloc::string::String;
use core::fmt;

/// Defines an owned-string newtype over `Box<str>` with the constructor,
/// accessor, `From`, and `Display` impls shared by every identifier kind.
macro_rules! str_newtype {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(Box<str>);

        impl $name {
            /// Wraps a string as this identifier.
            #[must_use]
            pub fn new(value: impl Into<Box<str>>) -> Self {
                Self(value.into())
            }

            /// Borrows the underlying string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.into())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value.into())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

str_newtype! {
    /// Name of a type constructor (e.g. `Int`, `List`, `Option`).
    Name
}

str_newtype! {
    /// Field label of a record type (e.g. `age`, `name`).
    Label
}

#[cfg(test)]
mod tests {
    use alloc::format;

    use super::{Label, Name};

    #[test]
    fn name_round_trips_through_as_str() {
        let name = Name::new("Option");
        assert_eq!(name.as_str(), "Option");
        assert_eq!(format!("{name}"), "Option");
    }

    #[test]
    fn labels_order_lexicographically() {
        // BTreeMap-backed records rely on this ordering for sorted display.
        let mut labels = [Label::new("name"), Label::new("age"), Label::new("id")];
        labels.sort();
        assert_eq!(
            labels,
            [Label::new("age"), Label::new("id"), Label::new("name")],
        );
    }
}

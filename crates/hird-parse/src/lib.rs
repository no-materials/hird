// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hird parser and CST construction.
//!
//! Provides a hand-rolled recursive descent parser that builds a
//! [`cstree`]-backed concrete syntax tree (CST) from a
//! [`hird_lex`] token stream. The CST preserves all source bytes
//! including whitespace and comments.
//!
//! # Quick start
//!
//! ```
//! let result = hird_parse::parse("let x = 42", 0);
//! assert!(result.is_ok());
//! ```

#![no_std]

extern crate alloc;

pub mod diagnostic;
pub mod syntax_kind;

mod parser;

pub use parser::{ParseResult, parse};
pub use syntax_kind::SyntaxKind;

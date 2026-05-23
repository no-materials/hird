// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hird lexer, token definitions, and span types.
//!
//! This crate provides a hand-written lexer that converts `&str` source
//! into a stream of [`Token`]s. It performs Unicode operator
//! normalisation (e.g. `->` and `\u{2192}` both produce [`TokenKind::Arrow`]) and
//! checks canonical naming conventions at lex time.
//!
//! # Quick start
//!
//! ```
//! use hird_lex::{Lexer, TokenKind};
//!
//! let mut lex = Lexer::new("let x = 42", 0);
//! assert_eq!(lex.next_token().kind, TokenKind::Let);
//! assert_eq!(lex.next_token().kind, TokenKind::Ident);
//! ```

#![no_std]

mod lexer;
mod token;

pub use lexer::Lexer;
pub use token::{LexError, Span, Token, TokenKind};

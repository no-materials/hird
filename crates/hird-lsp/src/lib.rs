// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! LSP server for Hirð.
//!
//! A [`tower_lsp`]-based language server over the compiler front end. Each
//! open document is compiled as a single-module program (parse, type-check);
//! the result is cached per file and rebuilt in full after every change.
//!
//! v0.1 scope:
//!
//! - **Diagnostics** on open and save: parse errors, then type errors and
//!   warnings, with source spans.
//! - **Hover**: the inferred type (and effect row, for functions) of the
//!   identifier or expression under the cursor.
//! - **Go-to-definition**: top-level functions, types and constructors,
//!   effects, tools (by marker or generated function name), actors and their
//!   message types, and supervisors — within the current file.
//!
//! Out of scope for v0.1: completion, rename, code actions, cross-file
//! analysis (imports do not resolve), and incremental compilation.

mod analysis;
mod line_index;
mod server;

pub use server::Backend;

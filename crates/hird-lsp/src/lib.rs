// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! LSP server for Hirð.
//!
//! A [`tower_lsp`]-based language server over the compiler front end. An
//! open document's directory is compiled as one program (every `.hird`
//! sibling is a module, so `use` imports resolve; open buffers stand in for
//! their files on disk); the result is cached per directory and rebuilt in
//! full after every change to one of its documents.
//!
//! v0.1 scope:
//!
//! - **Diagnostics** on open and save: parse errors, then type errors and
//!   warnings, with source spans. A save republishes every open document in
//!   the directory.
//! - **Hover**: the inferred type (and effect row, for functions) of the
//!   identifier or expression under the cursor, imported names included.
//! - **Go-to-definition**: top-level functions, types and constructors,
//!   effects, tools (by marker or generated function name), actors and their
//!   message types, and supervisors — in the current file or, for names
//!   imported with `use` (selectively or as `Qualifier.member`), in the
//!   sibling file that defines them, open or not.
//!
//! Out of scope for v0.1: completion, rename, code actions, and incremental
//! compilation.

mod analysis;
mod line_index;
mod server;

pub use server::Backend;

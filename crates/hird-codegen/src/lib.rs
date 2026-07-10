// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Erlang source emission from IR.
//!
//! [`emit_modules`] renders one lowered [`hird_ir::IrModule`] as a set of
//! `.erl` files that compile with stock `erlc`: a base module (plain
//! functions, every IR expression kind, handler-map threading for DI-style
//! effects, and dispatcher-routed tool calls) plus one `gen_server` behaviour
//! module per actor declaration. Supervisor declarations produce no forms yet
//! — their `supervisor` behaviour modules are emitted separately — and the
//! hand-written runtime modules (`hird_tool_dispatch`, …) are only
//! referenced, never generated.
//!
//! [`erlang_module_name`] is the module-file naming rule (`Planner` →
//! `hird_planner`), shared with callers that need to place or reference the
//! generated files.

#![no_std]

extern crate alloc;

mod emit;
mod names;

pub use emit::{EmittedModule, emit_modules};
pub use names::erlang_module_name;

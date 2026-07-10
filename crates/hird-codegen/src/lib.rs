// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Erlang source emission from IR.
//!
//! [`emit_module`] renders one lowered [`hird_ir::IrModule`] as one `.erl`
//! file that compiles with stock `erlc`: plain functions, every IR expression
//! kind, handler-map threading for DI-style effects, and dispatcher-routed
//! tool calls. Actor and supervisor declarations produce no forms here —
//! their `gen_server` / `supervisor` behaviour modules are emitted separately —
//! and the hand-written runtime modules (`hird_tool_dispatch`, …) are only
//! referenced, never generated.
//!
//! [`erlang_module_name`] is the module-file naming rule (`Planner` →
//! `hird_planner`), shared with callers that need to place or reference the
//! generated files.

#![no_std]

extern crate alloc;

mod emit;
mod names;

pub use emit::emit_module;
pub use names::erlang_module_name;

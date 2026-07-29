// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! MCP server for Hirð compiler introspection.
//!
//! A stdio-transport [Model Context Protocol] server exposing the compiler
//! pipeline (parse, type-check, lower to IR) as structured tools for LLM
//! agents. Messages are newline-delimited JSON-RPC 2.0; [`Server`] handles
//! one message at a time, so the binary is a plain blocking read loop.
//!
//! Tools:
//!
//! - `infer_type` — inferred type and effect row at a source location.
//! - `lookup_definition` — location, type, doc, and kind of a definition.
//! - `explain_effect_row` — a function's effect row, each effect explained.
//! - `render_ir_fragment` — the typed IR of one definition, as JSON.
//! - `explain_actor_protocol` — message constructors, state type, handler
//!   signatures, and effect summary of an actor.
//! - `emit_actor_effect_graph` — the actor/effect graph rooted at an actor,
//!   with supervisor relationships and transitive tool effects.
//! - `get_context_for_symbol` — token-budget-aware symbol summary.
//! - `get_context_budget` — approximate token costs per declaration category.
//!
//! Compilation is lazy: a file compiles on first query and the result is
//! cached until its source text changes. Invalid files, undefined names, and
//! parse or type errors all come back as structured tool errors, never as
//! protocol failures.

mod analysis;
mod server;
mod tools;

pub use server::Server;

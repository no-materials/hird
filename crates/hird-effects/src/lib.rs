// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hirð effect-handler lowering.
//!
//! The effect-row representation (`hird_types::EffectRow`, `hird_types::Effect`,
//! and row unification) lives in `hird-types` alongside the rest of the type
//! system, and a function body's effects are inferred in `hird-check`,
//! interleaved with type inference. This crate builds on both to lower `handle`
//! blocks (and to host any later pure effect-algebra helpers); that work is
//! still to come, so the crate is a placeholder for now.

#![no_std]

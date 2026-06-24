// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hirð effect inference and handler lowering.
//!
//! The effect-row representation (`hird_types::EffectRow`, `hird_types::Effect`,
//! and row unification) lives in `hird-types` alongside the rest of the type
//! system. This crate builds on it: inferring the effects a function body
//! performs and lowering `handle` blocks. Both are later work; the crate is a
//! placeholder for now.

#![no_std]

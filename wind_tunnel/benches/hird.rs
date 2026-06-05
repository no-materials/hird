// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hird benchmarks.

#![allow(missing_docs, reason = "criterion macros generate undocumented items")]

use criterion::{Criterion, criterion_group, criterion_main};

/// Placeholder benchmark exercising an empty iteration.
fn bench_placeholder(c: &mut Criterion) {
    c.bench_function("placeholder", |b| b.iter(|| {}));
}

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);

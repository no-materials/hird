// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Peak resident set size of this process, from `/proc` (Linux only).

use std::fs;

/// Resets the peak to the current RSS; whether the platform allowed it.
pub(crate) fn reset_peak() -> bool {
    fs::write("/proc/self/clear_refs", "5").is_ok()
}

/// Peak RSS in bytes since start or the last [`reset_peak`].
pub(crate) fn peak() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let kib = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))?
        .trim()
        .strip_suffix("kB")?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(kib * 1024)
}

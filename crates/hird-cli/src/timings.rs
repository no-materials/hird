// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `--timings`: wall time per phase and the pipeline's work counters,
//! printed to stderr as text or one line of JSON.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use hird_check::CheckStats;
use hird_parse::ParseStats;

/// Version of the `--timings=json` layout; bumped when a field changes
/// meaning or goes away.
const SCHEMA_VERSION: u32 = 1;

/// How `--timings` renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Format {
    /// Aligned tables.
    Text,
    /// One JSON object on one line.
    Json,
}

/// Wall time per phase and work counters for one command. Always recorded;
/// printed only on request.
#[derive(Debug, Default)]
pub(crate) struct Timings {
    /// Phases in run order, with their wall time.
    phases: Vec<(&'static str, Duration)>,
    /// Hirð modules loaded.
    pub(crate) modules: u64,
    /// Parser counters, summed over modules.
    pub(crate) parse: ParseStats,
    /// Checker counters, summed over modules.
    pub(crate) check: CheckStats,
    /// Bytes of Erlang generated from the program; the runtime and the boot
    /// module are not counted.
    pub(crate) erl_bytes: u64,
    /// Erlang modules handed to `erlc`.
    pub(crate) modules_compiled: u64,
    /// Erlang modules reused from an earlier build instead of compiled.
    pub(crate) modules_reused: u64,
    /// Emulators started: each `erlc` and each `erl` boots one.
    pub(crate) emulator_boots: u64,
}

impl Timings {
    /// Runs `f` as phase `name`, recording its wall time.
    pub(crate) fn phase<T>(&mut self, name: &'static str, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let out = f();
        self.phases.push((name, start.elapsed()));
        out
    }

    /// Every counter by its reported name, in report order.
    fn counters(&self) -> [(&'static str, u64); 13] {
        [
            ("modules", self.modules),
            ("tokens", self.parse.tokens),
            ("cst_nodes", self.parse.nodes),
            ("typed_nodes", self.check.typed_nodes),
            ("unify_calls", self.check.unify_calls),
            ("subst_slots", self.check.subst_slots),
            ("exhaustiveness_rows", self.check.exhaustiveness_rows),
            (
                "exhaustiveness_witnesses",
                self.check.exhaustiveness_witnesses,
            ),
            ("effect_row_merges", self.check.effect_row_merges),
            ("erl_bytes", self.erl_bytes),
            ("modules_compiled", self.modules_compiled),
            ("modules_reused", self.modules_reused),
            ("emulator_boots", self.emulator_boots),
        ]
    }

    /// Prints the report to stderr in `format`.
    pub(crate) fn report(&self, format: Format) {
        match format {
            Format::Text => eprint!("{}", self.text()),
            Format::Json => eprintln!("{}", self.json()),
        }
    }

    /// The text form: phases with a total, then counters.
    fn text(&self) -> String {
        let mut out = String::from("timings:\n");
        for (name, wall) in &self.phases {
            let _ = writeln!(out, "  {name:<26} {:>10}", millis(*wall));
        }
        let total: Duration = self.phases.iter().map(|(_, wall)| *wall).sum();
        let _ = writeln!(out, "  {:<26} {:>10}", "total", millis(total));
        out.push_str("counters:\n");
        for (name, value) in self.counters() {
            let _ = writeln!(out, "  {name:<26} {value:>10}");
        }
        out
    }

    /// The JSON form:
    /// `{"schema_version":1,"phases":[{"name":…,"wall_us":…},…],"counters":{…}}`.
    fn json(&self) -> serde_json::Value {
        let phases: Vec<serde_json::Value> = self
            .phases
            .iter()
            .map(|(name, wall)| {
                serde_json::json!({
                    "name": name,
                    "wall_us": u64::try_from(wall.as_micros()).unwrap_or(u64::MAX),
                })
            })
            .collect();
        let counters: serde_json::Map<String, serde_json::Value> = self
            .counters()
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.into()))
            .collect();
        serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "phases": phases,
            "counters": counters,
        })
    }
}

/// `wall` as `<n.n> ms`.
fn millis(wall: Duration) -> String {
    format!("{:.1} ms", wall.as_secs_f64() * 1e3)
}

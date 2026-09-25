// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hirð compiler CLI: type-check, compile to Erlang/BEAM, run, and dump the
//! typed AST or the actor/effect graph.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

mod build;
mod demo;
mod pipeline;
mod report;
mod text;
mod timings;

use timings::Timings;

/// A failed subcommand: either a message still to print, or diagnostics
/// already rendered to stderr.
pub(crate) enum Failure {
    /// The failure message, printed as `hird: <message>`.
    Message(String),
    /// Diagnostics were already rendered; only the exit code remains.
    Reported,
}

/// Builds a [`Failure::Message`] with `format!` arguments.
macro_rules! fail {
    ($($arg:tt)*) => { $crate::Failure::Message(format!($($arg)*)) };
}
pub(crate) use fail;

/// The Hirð compiler.
#[derive(Parser)]
#[command(name = "hird", version, about = "The Hir\u{f0} compiler")]
struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    command: Command,
}

/// The CLI subcommands.
#[derive(Subcommand)]
enum Command {
    /// Type-check source files and report diagnostics.
    Check {
        /// A `.hird` file, or a directory of independent `.hird` modules.
        input: PathBuf,
        /// Timing and work-counter reporting.
        #[command(flatten)]
        timings: TimingsArg,
    },
    /// Compile to Erlang source and .beam files.
    Build {
        /// A `.hird` file, or a directory of independent `.hird` modules.
        input: PathBuf,
        /// The build output directory.
        #[arg(short, long, default_value = "_build/hird")]
        out_dir: PathBuf,
        /// Append the audit stream to this file instead of stdout.
        #[arg(long)]
        audit_file: Option<PathBuf>,
        /// Replay tool calls from this recorded audit log instead of
        /// dispatching to handlers.
        #[arg(long)]
        replay: Option<PathBuf>,
        /// Timing and work-counter reporting.
        #[command(flatten)]
        timings: TimingsArg,
    },
    /// Build, then run on BEAM (requires a module defining `fn main`).
    Run {
        /// A `.hird` file, or a directory of independent `.hird` modules.
        input: PathBuf,
        /// The build output directory.
        #[arg(short, long, default_value = "_build/hird")]
        out_dir: PathBuf,
        /// Append the audit stream to this file instead of stdout.
        #[arg(long)]
        audit_file: Option<PathBuf>,
        /// Replay tool calls from this recorded audit log instead of
        /// dispatching to handlers.
        #[arg(long)]
        replay: Option<PathBuf>,
        /// Arguments for `main` (reserved; not supported in v0.1).
        #[arg(last = true)]
        args: Vec<String>,
        /// Timing and work-counter reporting.
        #[command(flatten)]
        timings: TimingsArg,
    },
    /// Record one run of the built-in demo and replay it against variants.
    ///
    /// Writes the demo planner and two edited variants of it into the
    /// output directory, records one run, replays that recording against
    /// all three, and prints where each one parts from the recording.
    Demo {
        /// The directory the demo's sources, recording, and build output
        /// go in.
        #[arg(short, long, default_value = "_build/hird-demo")]
        out_dir: PathBuf,
    },
    /// Dump the typed AST of one file.
    EmitAst {
        /// A `.hird` file.
        input: PathBuf,
        /// Emit JSON instead of pretty-printed source.
        #[arg(long)]
        json: bool,
    },
    /// Dump the actor/effect graph.
    EmitEffectGraph {
        /// A `.hird` file, or a directory of independent `.hird` modules.
        input: PathBuf,
        /// Emit structured JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Compare the effect graph against a committed baseline; exit nonzero
    /// when effect reach widens.
    EffectDiff {
        /// A baseline written by `emit-effect-graph --json`.
        baseline: PathBuf,
        /// A `.hird` file, or a directory of independent `.hird` modules.
        input: PathBuf,
        /// Emit the structured JSON report instead of text.
        #[arg(long)]
        json: bool,
        /// Exit nonzero on any change, not only widening, so the baseline
        /// must be regenerated whenever the graph moves.
        #[arg(long)]
        exact: bool,
    },
}

/// `--timings[=FORMAT]`, shared by `check`, `build`, and `run`.
#[derive(Args)]
struct TimingsArg {
    /// Print wall time per phase and work counters to stderr when done.
    ///
    /// `--timings` prints text; `--timings=json` prints one JSON object on
    /// one line, with `schema_version`, `phases` (each a `name` and its
    /// `wall_us`), and `counters` (by name).
    ///
    /// Phases, in the order a command runs them: `load` (read the sources),
    /// `parse` (lexing included), `check`, `lower`, `emit` (generate
    /// Erlang), `write` (the `.erl` files), `erlc`, and `run` (the
    /// emulator).
    ///
    /// Counters depend only on the sources and the paths they are named by:
    /// `modules`, `tokens`, `cst_nodes`, `typed_nodes`, `unify_calls`
    /// (recursive calls included), `subst_slots` (type and row variables),
    /// `exhaustiveness_rows` (pattern-matrix rows visited),
    /// `exhaustiveness_witnesses` (witness rows built), `effect_row_merges`
    /// (effects visited merging call rows into body rows), `erl_bytes`
    /// (generated Erlang; the runtime and boot module excluded),
    /// `modules_compiled` and `modules_reused` (Erlang modules), and
    /// `emulator_boots`.
    #[arg(
        long,
        value_name = "FORMAT",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "text"
    )]
    timings: Option<timings::Format>,
}

impl TimingsArg {
    /// Prints `timings` in the requested format, if any.
    fn report(&self, timings: &Timings) {
        if let Some(format) = self.timings {
            timings.report(format);
        }
    }
}

fn main() -> ExitCode {
    match dispatch(Cli::parse().command) {
        Ok(code) => code,
        Err(Failure::Message(message)) => {
            eprintln!("hird: {message}");
            ExitCode::FAILURE
        }
        Err(Failure::Reported) => ExitCode::FAILURE,
    }
}

/// Runs one subcommand, returning its exit code.
fn dispatch(command: Command) -> Result<ExitCode, Failure> {
    match command {
        Command::Check {
            input,
            timings: report,
        } => {
            let mut timings = Timings::default();
            let modules = check_input(&input, &mut timings)?;
            eprintln!("checked {} module(s)", modules.len());
            report.report(&timings);
            Ok(ExitCode::SUCCESS)
        }
        Command::Build {
            input,
            out_dir,
            audit_file,
            replay,
            timings: report,
        } => {
            let mut timings = Timings::default();
            build_input(
                &input,
                &out_dir,
                audit_file.as_deref(),
                replay.as_deref(),
                &mut timings,
            )?;
            report.report(&timings);
            Ok(ExitCode::SUCCESS)
        }
        Command::Run {
            input,
            out_dir,
            audit_file,
            replay,
            args,
            timings: report,
        } => {
            if !args.is_empty() {
                return Err(fail!(
                    "arguments to `main` are reserved and not supported in v0.1"
                ));
            }
            let mut timings = Timings::default();
            let output = build_input(
                &input,
                &out_dir,
                audit_file.as_deref(),
                replay.as_deref(),
                &mut timings,
            )?;
            let status = build::run(&output, &mut timings)?;
            report.report(&timings);
            Ok(u8::try_from(status).map_or(ExitCode::FAILURE, ExitCode::from))
        }
        Command::Demo { out_dir } => {
            demo::run(&out_dir)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::EmitAst { input, json } => {
            if input.is_dir() {
                return Err(fail!("emit-ast takes a single .hird file"));
            }
            let modules = check_input(&input, &mut Timings::default())?;
            let ir = modules[0].lower();
            if json {
                let rendered = ir
                    .to_json_pretty()
                    .map_err(|e| fail!("cannot serialize IR: {e}"))?;
                println!("{rendered}");
            } else {
                print!("{}", hird_ir::pretty_print(&ir));
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::EmitEffectGraph { input, json } => {
            let modules = check_input(&input, &mut Timings::default())?;
            let graphs: Vec<(PathBuf, hird_ir::EffectGraph)> = modules
                .iter()
                .map(|m| (m.path.clone(), hird_ir::effect_graph(&m.lower())))
                .collect();
            if json {
                let program = hird_ir::ProgramGraph::new(graphs.into_iter().map(|(_, g)| g));
                let rendered = serde_json::to_string_pretty(&program)
                    .map_err(|e| fail!("cannot serialize effect graph: {e}"))?;
                println!("{rendered}");
            } else {
                for (i, (path, graph)) in graphs.iter().enumerate() {
                    if i > 0 {
                        println!();
                    }
                    print!("{}", text::render_graph(graph, path));
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::EffectDiff {
            baseline,
            input,
            json,
            exact,
        } => {
            let baseline = load_baseline(&baseline)?;
            let modules = check_input(&input, &mut Timings::default())?;
            let current = hird_ir::ProgramGraph::new(
                modules.iter().map(|m| hird_ir::effect_graph(&m.lower())),
            );
            let report = hird_policy::diff(&baseline, &current);
            if json {
                let rendered = serde_json::to_string_pretty(&report)
                    .map_err(|e| fail!("cannot serialize diff report: {e}"))?;
                println!("{rendered}");
            } else {
                print!("{}", text::render_diff(&report));
            }
            let fails = report.widens || (exact && !report.changes.is_empty());
            Ok(if fails {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
    }
}

/// Reads a baseline written by `emit-effect-graph --json`, refusing one
/// from another schema version.
fn load_baseline(path: &Path) -> Result<hird_ir::ProgramGraph, Failure> {
    let text = fs::read_to_string(path)
        .map_err(|e| fail!("cannot read baseline `{}`: {e}", path.display()))?;
    let graph: hird_ir::ProgramGraph = serde_json::from_str(&text)
        .map_err(|e| fail!("`{}` is not an effect-graph baseline: {e}", path.display()))?;
    if graph.schema_version != hird_ir::EFFECT_GRAPH_SCHEMA_VERSION {
        return Err(fail!(
            "baseline `{}` has schema version {}; this hird reads version {}",
            path.display(),
            graph.schema_version,
            hird_ir::EFFECT_GRAPH_SCHEMA_VERSION
        ));
    }
    Ok(graph)
}

/// Loads, parses, and checks `input`, recording the `load`, `parse`, and
/// `check` phases in `timings`.
fn check_input(
    input: &Path,
    timings: &mut Timings,
) -> Result<Vec<pipeline::CheckedModule>, Failure> {
    let modules = timings.phase("load", || pipeline::load(input))?;
    pipeline::parse_and_check(modules, timings)
}

/// Checks `input` and builds it into `out_dir`, recording every phase in
/// `timings`; the audit stream goes to `audit_file` when given, stdout
/// otherwise, and `replay` makes the boot module replay tool calls from
/// that recorded log.
fn build_input(
    input: &Path,
    out_dir: &Path,
    audit_file: Option<&Path>,
    replay: Option<&Path>,
    timings: &mut Timings,
) -> Result<build::BuildOutput, Failure> {
    let modules = check_input(input, timings)?;
    let lowered: Vec<(PathBuf, hird_ir::IrModule)> = timings.phase("lower", || {
        modules
            .iter()
            .map(|m| (m.path.clone(), m.lower()))
            .collect()
    });
    build::build(&lowered, out_dir, audit_file, replay, timings)
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hirð compiler CLI: type-check, compile to Erlang/BEAM, run, and dump the
//! typed AST or the actor/effect graph.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod build;
mod pipeline;
mod report;
mod text;

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
    },
    /// Compile to Erlang source and .beam files.
    Build {
        /// A `.hird` file, or a directory of independent `.hird` modules.
        input: PathBuf,
        /// The build output directory.
        #[arg(short, long, default_value = "_build/hird")]
        out_dir: PathBuf,
    },
    /// Build, then run on BEAM (requires a module defining `fn main`).
    Run {
        /// A `.hird` file, or a directory of independent `.hird` modules.
        input: PathBuf,
        /// The build output directory.
        #[arg(short, long, default_value = "_build/hird")]
        out_dir: PathBuf,
        /// Arguments for `main` (reserved; not supported in v0.1).
        #[arg(last = true)]
        args: Vec<String>,
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
        Command::Check { input } => {
            let modules = pipeline::parse_and_check(pipeline::load(&input)?)?;
            eprintln!("checked {} module(s)", modules.len());
            Ok(ExitCode::SUCCESS)
        }
        Command::Build { input, out_dir } => {
            build_input(&input, &out_dir)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Run {
            input,
            out_dir,
            args,
        } => {
            if !args.is_empty() {
                return Err(fail!(
                    "arguments to `main` are reserved and not supported in v0.1"
                ));
            }
            let output = build_input(&input, &out_dir)?;
            let status = build::run(&output)?;
            Ok(u8::try_from(status).map_or(ExitCode::FAILURE, ExitCode::from))
        }
        Command::EmitAst { input, json } => {
            if input.is_dir() {
                return Err(fail!("emit-ast takes a single .hird file"));
            }
            let modules = pipeline::parse_and_check(pipeline::load(&input)?)?;
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
            let modules = pipeline::parse_and_check(pipeline::load(&input)?)?;
            let graphs: Vec<(PathBuf, hird_ir::EffectGraph)> = modules
                .iter()
                .map(|m| (m.path.clone(), hird_ir::effect_graph(&m.lower())))
                .collect();
            if json {
                let rendered = if let [(_, only)] = graphs.as_slice() {
                    serde_json::to_string_pretty(only)
                } else {
                    serde_json::to_string_pretty(&graphs.iter().map(|(_, g)| g).collect::<Vec<_>>())
                }
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
    }
}

/// Checks `input` and builds it into `out_dir`.
fn build_input(input: &Path, out_dir: &Path) -> Result<build::BuildOutput, Failure> {
    let modules = pipeline::parse_and_check(pipeline::load(input)?)?;
    let lowered: Vec<(PathBuf, hird_ir::IrModule)> = modules
        .iter()
        .map(|m| (m.path.clone(), m.lower()))
        .collect();
    build::build(&lowered, out_dir)
}

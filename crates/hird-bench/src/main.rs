// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `hird-bench`: generates a corpus of one shape at one size, then reports
//! per-stage medians and peak RSS, and end-to-end `hird` subcommand timings
//! with the emulators each one boots.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

use clap::Parser;

mod beam;
mod corpus;
mod front;
mod rss;

use beam::{BootCounter, Boots, BuildDir};
use corpus::{Shape, SourceModule};

/// Benchmarks the Hirð compiler on a generated corpus.
#[derive(Parser)]
#[command(
    name = "hird-bench",
    about = "Benchmark the Hir\u{f0} compiler on a generated corpus"
)]
struct Args {
    /// The corpus shape.
    shape: Shape,
    /// How far the shape scales: functions, modules, record fields, tools,
    /// tuple width, chain links, or actors.
    #[arg(long)]
    size: usize,
    /// Generator seed; the same seed writes the same bytes.
    #[arg(long, default_value_t = 0)]
    seed: u64,
    /// Timed runs per stage and per command; medians are reported.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..))]
    runs: u32,
    /// Directory the corpus and build outputs are written under.
    #[arg(long, default_value = "_build/hird-bench")]
    out: PathBuf,
    /// The `hird` binary for the BEAM and CLI timings [default: the `hird`
    /// beside this binary].
    #[arg(long)]
    hird: Option<PathBuf>,
    /// Time only the in-process front end: no `hird` binary, no Erlang.
    #[arg(long)]
    front_only: bool,
}

fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("hird-bench: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Generates, writes, and benchmarks the corpus `args` names.
fn run(args: &Args) -> Result<(), String> {
    let sizes = args.shape.sizes();
    if !sizes.contains(&args.size) {
        let bound = match *sizes.end() {
            usize::MAX => format!("at least {}", sizes.start()),
            end => format!("{} to {end}", sizes.start()),
        };
        return Err(format!("{} takes a size of {bound}", args.shape.name()));
    }
    // An explicit `--hird` must exist; the default may be missing, which
    // only skips the timings that need it.
    let hird = match &args.hird {
        Some(path) if !args.front_only && !path.is_file() => {
            return Err(format!("no hird binary at `{}`", path.display()));
        }
        Some(path) => path.clone(),
        None => env::current_exe()
            .map_err(|e| format!("cannot locate hird-bench: {e}"))?
            .with_file_name(format!("hird{}", env::consts::EXE_SUFFIX)),
    };
    if cfg!(debug_assertions) {
        eprintln!("hird-bench: warning: debug build; pass --release for representative numbers");
    }

    let modules = corpus::generate(args.shape, args.size, args.seed);
    let root = args.out.join(format!(
        "{}-{}-s{}",
        args.shape.name(),
        args.size,
        args.seed
    ));
    let src = root.join("src");
    write_corpus(&src, &modules)?;
    let lines: usize = modules.iter().map(|m| m.source.lines().count()).sum();
    let bytes: usize = modules.iter().map(|m| m.source.len()).sum();
    println!(
        "hird-bench {} --size {} --seed {}: {} run(s) each",
        args.shape.name(),
        args.size,
        args.seed,
        args.runs
    );
    println!(
        "corpus: {} module(s), {lines} lines, {:.1} KiB, in {}",
        modules.len(),
        kib(bytes),
        src.display()
    );

    front_end(&modules, &src, args.runs)?;
    if args.front_only {
        return Ok(());
    }

    if !hird.is_file() {
        println!(
            "\nBEAM and CLI timings skipped: no hird binary at `{}`; build it \
             (cargo build --release -p hird-cli) or pass --hird",
            hird.display()
        );
        return Ok(());
    }
    let erlang = beam::find_on_path("erlc").is_some() && beam::find_on_path("erl").is_some();
    if erlang {
        beam_stages(&hird, &src, &root, args.runs)?;
    } else {
        println!("\nerlc and boot skipped: Erlang/OTP is not on PATH");
    }
    cli(&hird, &src, &root, args.runs, erlang)
}

/// Writes `modules` into `src`, replacing whatever an earlier run left there:
/// a stale file would join the program as an extra module.
fn write_corpus(src: &Path, modules: &[SourceModule]) -> Result<(), String> {
    if src.exists() {
        fs::remove_dir_all(src).map_err(|e| format!("cannot clear `{}`: {e}", src.display()))?;
    }
    fs::create_dir_all(src).map_err(|e| format!("cannot create `{}`: {e}", src.display()))?;
    for module in modules {
        let path = src.join(&module.file);
        fs::write(&path, &module.source)
            .map_err(|e| format!("cannot write `{}`: {e}", path.display()))?;
    }
    Ok(())
}

/// Times the in-process stages over `runs` passes and prints their table.
fn front_end(modules: &[SourceModule], src: &Path, runs: u32) -> Result<(), String> {
    let paths: Vec<String> = modules
        .iter()
        .map(|m| src.join(&m.file).display().to_string())
        .collect();
    let passes = (0..runs)
        .map(|_| front::pass(modules, &paths))
        .collect::<Result<Vec<_>, _>>()?;
    let first = &passes[0];
    println!(
        "\nfront end, in process ({} tokens, {} Erlang module(s) emitted)",
        first.tokens, first.erlang_modules
    );
    // Peaks come from the first pass: later passes reuse heap the allocator
    // kept, which would lift every stage to the first pass's high-water mark.
    header("stage", "peak RSS");
    for (i, stage) in front::STAGES.iter().enumerate() {
        let times: Vec<Duration> = passes.iter().map(|p| p.stages[i].time).collect();
        row(
            stage,
            &times,
            &first.stages[i].peak.map_or_else(|| "n/a".to_owned(), mib),
        );
    }
    let totals: Vec<Duration> = passes
        .iter()
        .map(|p| p.stages.iter().map(|s| s.time).sum())
        .collect();
    let peak = first.stages.iter().filter_map(|s| s.peak).max();
    row(
        "total",
        &totals,
        &peak.map_or_else(|| "n/a".to_owned(), mib),
    );
    Ok(())
}

/// Builds the corpus once with `hird build`, then times `erlc` over the
/// files it wrote and the emulator running the result.
fn beam_stages(hird: &Path, src: &Path, root: &Path, runs: u32) -> Result<(), String> {
    let build = BuildDir::build(hird, src, &root.join("build"))?;
    let erlc = (0..runs)
        .map(|_| build.erlc())
        .collect::<Result<Vec<_>, _>>()?;
    let boot = (0..runs)
        .map(|_| build.boot())
        .collect::<Result<Vec<_>, _>>()?;
    println!(
        "\nBEAM: erlc over the {} .erl files `hird build` writes; boot runs the result on erl",
        build.erl_files.len()
    );
    header("stage", "");
    row("erlc", &erlc, "");
    row("boot", &boot, "");
    Ok(())
}

/// Times the `hird` subcommands end to end, after one untimed run of each
/// that counts the emulators it boots (on Unix).
fn cli(hird: &Path, src: &Path, root: &Path, runs: u32, erlang: bool) -> Result<(), String> {
    let counter = BootCounter::install(&root.join("shims"))?;
    let mut commands: Vec<(&str, Vec<OsString>)> =
        vec![("check", vec!["check".into(), src.into()])];
    if erlang {
        for sub in ["build", "run"] {
            let out = root.join(format!("cli-{sub}"));
            commands.push((sub, vec![sub.into(), src.into(), "-o".into(), out.into()]));
        }
        let out = root.join("cli-demo");
        commands.push(("demo", vec!["demo".into(), "-o".into(), out.into()]));
    }
    // A fresh `Command` per run: the boot count's `PATH` must not carry
    // over into the timed runs.
    let command = |args: &[OsString]| {
        let mut command = Command::new(hird);
        command.args(args);
        command
    };

    println!("\nCLI, `{}` end to end", hird.display());
    header("command", "boots");
    for (name, args) in &commands {
        let boots = match &counter {
            Some(counter) => Some(counter.count(&mut command(args))?),
            None => {
                beam::time(&mut command(args))?;
                None
            }
        };
        let times = (0..runs)
            .map(|_| beam::time(&mut command(args)))
            .collect::<Result<Vec<_>, _>>()?;
        row(
            name,
            &times,
            &boots.map_or_else(|| "n/a".to_owned(), boot_count),
        );
    }
    if !erlang {
        println!("build, run and demo skipped: Erlang/OTP is not on PATH");
    }
    Ok(())
}

/// Prints a table header: the row label's column name, then the extra
/// column's.
fn header(label: &str, extra: &str) {
    let line = format!(
        "  {label:<10} {:>10} {:>10} {:>10}  {extra}",
        "median", "min", "max"
    );
    println!("{}", line.trim_end());
}

/// Prints one table row: median, min, and max of `times`, then `extra`.
fn row(label: &str, times: &[Duration], extra: &str) {
    let mut sorted = times.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    let median = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2
    };
    let line = format!(
        "  {label:<10} {:>10} {:>10} {:>10}  {extra}",
        duration(median),
        duration(sorted[0]),
        duration(sorted[n - 1])
    );
    println!("{}", line.trim_end());
}

/// `d` in the largest unit that keeps it at or above 1.
fn duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs >= 1.0 {
        format!("{secs:.2} s")
    } else if secs >= 1e-3 {
        format!("{:.1} ms", secs * 1e3)
    } else {
        format!("{:.0} µs", secs * 1e6)
    }
}

/// `bytes` in KiB.
fn kib(bytes: usize) -> f64 {
    bytes as f64 / 1024.0
}

/// `bytes` as `<n> MiB`.
fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

/// `<total> (<erlc> erlc, <erl> erl)`, or `0`.
fn boot_count(boots: Boots) -> String {
    match (boots.erlc, boots.erl) {
        (0, 0) => "0".to_owned(),
        (erlc, erl) => format!("{} ({erlc} erlc, {erl} erl)", erlc + erl),
    }
}

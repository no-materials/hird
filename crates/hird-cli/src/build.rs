// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Erlang emission and BEAM compilation: write generated and runtime `.erl`
//! files into the build directory, generate the boot module, and drive
//! `erlc`/`erl`.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::Mutex;

use hird_codegen::erlang_module_name;
use hird_ir::{IrDecl, IrFnDef, IrModule};
use hird_types::Type;

use crate::cache::BeamCache;
use crate::timings::Timings;
use crate::{Failure, fail};

/// The hand-written Erlang runtime library, embedded so the compiled binary
/// is self-contained.
const RUNTIME: [(&str, &str); 9] = [
    (
        "hird_actor",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../runtime/hird_actor.erl"
        )),
    ),
    (
        "hird_audit",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../runtime/hird_audit.erl"
        )),
    ),
    (
        "hird_clock",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../runtime/hird_clock.erl"
        )),
    ),
    (
        "hird_handlers",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../runtime/hird_handlers.erl"
        )),
    ),
    (
        "hird_replay",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../runtime/hird_replay.erl"
        )),
    ),
    (
        "hird_stand",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../runtime/hird_stand.erl"
        )),
    ),
    (
        "hird_sup_util",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../runtime/hird_sup_util.erl"
        )),
    ),
    (
        "hird_tool_dispatch",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../runtime/hird_tool_dispatch.erl"
        )),
    ),
    (
        "hird_types",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../runtime/hird_types.erl"
        )),
    ),
];

/// The generated boot module's name.
const BOOT_MODULE: &str = "hird_boot";

/// The options every `erlc` run takes: `+deterministic` keeps build paths
/// out of a `.beam`, so one compiled in another build directory is the
/// same file.
const ERLC_OPTIONS: [&str; 1] = ["+deterministic"];

/// The advice printed when `erlc`/`erl` are not on `PATH`.
const ERLANG_MISSING: &str =
    "Erlang/OTP not found. Install from https://www.erlang.org/ or use asdf/nix.";

/// A compiled program's entry point: the base module defining `fn main`.
pub(crate) struct EntryPoint {
    /// The Erlang name of the module defining `main`.
    pub(crate) module: String,
    /// Whether `main` threads the handler map (non-empty effect row).
    pub(crate) takes_map: bool,
}

/// The result of writing and compiling a build directory.
pub(crate) struct BuildOutput {
    /// The build output directory.
    pub(crate) out_dir: PathBuf,
    /// The program entry point, when some module defines `fn main`.
    pub(crate) entry: Option<EntryPoint>,
}

/// Emits `modules` (paired with their source paths) plus the runtime and
/// boot module into `out_dir`, then gives every module a `.beam`: reused
/// from the build cache when it holds one, compiled by one `erlc` run
/// otherwise. Records the `emit`, `write`, and `erlc` phases and their
/// counters in `timings`.
pub(crate) fn build(
    modules: &[(PathBuf, IrModule)],
    out_dir: &Path,
    timings: &mut Timings,
) -> Result<BuildOutput, Failure> {
    let entry = find_entry_point(modules)?;

    let emitted = timings.phase("emit", || emit(modules))?;
    timings.erl_bytes = emitted.iter().map(|(_, source)| source.len() as u64).sum();

    let boot = entry.as_ref().map(|entry| {
        let table_modules: Vec<String> = modules
            .iter()
            .filter(|(_, m)| hird_codegen::has_signature_table(m))
            .map(|(_, m)| erlang_module_name(&m.name))
            .collect();
        boot_module(entry, &table_modules)
    });
    let sources: Vec<(&str, &str)> = emitted
        .iter()
        .map(|(name, source)| (name.as_str(), source.as_str()))
        .chain(RUNTIME)
        .chain(boot.as_deref().map(|source| (BOOT_MODULE, source)))
        .collect();

    timings.phase("write", || -> Result<(), Failure> {
        fs::create_dir_all(out_dir)
            .map_err(|e| fail!("cannot create `{}`: {e}", out_dir.display()))?;
        for (name, source) in &sources {
            write_erl(out_dir, name, source)?;
        }
        Ok(())
    })?;

    let beams = timings.phase("erlc", || compile(&sources, out_dir))?;
    timings.modules_compiled = beams.compiled;
    timings.modules_reused = beams.reused;
    if beams.compiled > 0 {
        timings.emulator_boots += 1;
    }

    match beams.reused {
        0 => eprintln!(
            "compiled {} module(s) to {}",
            beams.compiled,
            out_dir.display()
        ),
        reused => eprintln!(
            "compiled {} module(s) to {}, reused {reused} from the build cache",
            beams.compiled,
            out_dir.display()
        ),
    }
    Ok(BuildOutput {
        out_dir: out_dir.to_path_buf(),
        entry,
    })
}

/// Generates Erlang for `modules`: one `(module name, source)` per emitted
/// module, refusing a generated name that collides with another or with
/// the runtime.
fn emit(modules: &[(PathBuf, IrModule)]) -> Result<Vec<(String, String)>, Failure> {
    let mut emitted: Vec<(String, String)> = Vec::new();
    let mut origin: std::collections::BTreeMap<String, PathBuf> = RUNTIME
        .iter()
        .map(|(name, _)| ((*name).to_owned(), PathBuf::from("the Hirð runtime")))
        .collect();
    origin.insert(BOOT_MODULE.to_owned(), PathBuf::from("the Hirð runtime"));
    for (path, module) in modules {
        // The file name alone: generated Erlang must not depend on where
        // the source sits, or the same module built elsewhere misses the
        // build cache.
        let file_name = path.file_name().unwrap_or(path.as_os_str());
        for out in hird_codegen::emit_modules(module, &file_name.to_string_lossy()) {
            if let Some(first) = origin.insert(out.name.clone(), path.clone()) {
                return Err(fail!(
                    "generated module name `{}` collides: `{}` and `{}`",
                    out.name,
                    first.display(),
                    path.display()
                ));
            }
            emitted.push((out.name, out.source));
        }
    }
    Ok(emitted)
}

/// How a build's modules got their `.beam` files.
struct Beams {
    /// Modules handed to `erlc`.
    compiled: u64,
    /// Modules copied from the build cache.
    reused: u64,
}

/// Gives each of `sources`, already written to `out_dir`, a `.beam` there:
/// copied from the build cache when it holds one, else compiled by a single
/// `erlc` run and then stored in the cache.
fn compile(sources: &[(&str, &str)], out_dir: &Path) -> Result<Beams, Failure> {
    let cache = BeamCache::open(&ERLC_OPTIONS);
    let beam = |name: &str| out_dir.join(format!("{name}.beam"));
    let mut misses: Vec<(&str, Option<String>)> = Vec::new();
    for &(name, source) in sources {
        let key = cache.as_ref().map(|cache| cache.key(source));
        let hit = cache
            .as_ref()
            .zip(key.as_deref())
            .is_some_and(|(cache, key)| cache.fetch(key, &beam(name)));
        if !hit {
            misses.push((name, key));
        }
    }
    let beams = Beams {
        compiled: misses.len() as u64,
        reused: (sources.len() - misses.len()) as u64,
    };
    if misses.is_empty() {
        return Ok(beams);
    }

    let output = Command::new("erlc")
        .args(ERLC_OPTIONS)
        .arg("-o")
        .arg(out_dir)
        .args(
            misses
                .iter()
                .map(|(name, _)| out_dir.join(format!("{name}.erl"))),
        )
        .output()
        .map_err(erlang_unavailable)?;
    io::Write::write_all(&mut io::stderr(), &output.stdout)
        .and_then(|()| io::Write::write_all(&mut io::stderr(), &output.stderr))
        .map_err(|e| fail!("cannot write erlc output: {e}"))?;
    if !output.status.success() {
        return Err(fail!("erlc failed with {}", output.status));
    }
    if let Some(cache) = &cache {
        for (name, key) in &misses {
            if let Some(key) = key {
                cache.store(key, &beam(name));
            }
        }
    }
    Ok(beams)
}

/// Runs a built program on BEAM through the generated boot module, returning
/// the emulator's exit status.
///
/// `hird run` owns the program's stop channel: it keeps the emulator's stdin
/// as a pipe, tells the runtime so (`-hird_stop stdin`), and closes the pipe
/// on Ctrl-C or termination (SIGTERM/SIGHUP on Unix, console close on
/// Windows). A standing program sees end of file, shuts its trees down and
/// syncs the audit stream before halting; any other program stops. The
/// console delivers Ctrl-C to the emulator too, where the BEAM's break
/// handler would act on it, so the emulator is told to ignore it (`+Bi`) and
/// the pipe is the only stop path. Nothing here is platform-specific.
/// `boot_args` are the boot module's init arguments, from [`boot_args`].
pub(crate) fn run(
    build: &BuildOutput,
    boot_args: &[OsString],
    timings: &mut Timings,
) -> Result<i32, Failure> {
    let entry_module = match &build.entry {
        Some(_) => BOOT_MODULE,
        None => {
            return Err(fail!(
                "no `fn main` found: `hird run` needs a module defining `fn main() \u{2192} ()`"
            ));
        }
    };
    timings.emulator_boots += 1;
    let status = timings.phase("run", || -> Result<_, Failure> {
        let mut child = Command::new("erl")
            .arg("+Bi")
            .arg("-noshell")
            .arg("-pa")
            .arg(&build.out_dir)
            .arg("-s")
            .arg(entry_module)
            .arg("run")
            .arg("-hird_stop")
            .arg("stdin")
            .args(boot_args)
            .stdin(Stdio::piped())
            .spawn()
            .map_err(erlang_unavailable)?;
        let stdin = child.stdin.take().expect("the emulator's stdin is piped");
        stop_on_termination(stdin)?;
        child
            .wait()
            .map_err(|e| fail!("cannot wait for Erlang/OTP: {e}"))
    })?;
    Ok(status.code().unwrap_or(1))
}

/// The boot module's init arguments: `-hird_audit_file <path>` to append
/// the audit stream to `audit_file`, `-hird_replay <path>` to replay that
/// log. Paths are made absolute, so none reads as an emulator flag, and
/// must be UTF-8.
pub(crate) fn boot_args(
    audit_file: Option<&Path>,
    replay: Option<&Path>,
) -> Result<Vec<OsString>, Failure> {
    let mut args = Vec::new();
    for (flag, what, path) in [
        ("-hird_audit_file", "audit file", audit_file),
        ("-hird_replay", "replay log", replay),
    ] {
        let Some(path) = path else { continue };
        if path.to_str().is_none() {
            return Err(fail!("{what} path `{}` is not valid UTF-8", path.display()));
        }
        let path = std::path::absolute(path)
            .map_err(|e| fail!("cannot resolve {what} path `{}`: {e}", path.display()))?;
        args.extend([OsString::from(flag), path.into_os_string()]);
    }
    Ok(args)
}

/// Installs the handler that closes the emulator's stdin — the stop
/// channel — on Ctrl-C or a termination request. Closing an already
/// closed pipe is a no-op, so a second signal is harmless.
fn stop_on_termination(stdin: ChildStdin) -> Result<(), Failure> {
    let stdin = Mutex::new(Some(stdin));
    ctrlc::set_handler(move || {
        if let Ok(mut slot) = stdin.lock() {
            drop(slot.take());
        }
    })
    .map_err(|e| fail!("cannot install the termination handler: {e}"))
}

/// Maps a process-spawn error to the missing-Erlang advice when the binary
/// is absent from `PATH`.
fn erlang_unavailable(error: io::Error) -> Failure {
    if error.kind() == io::ErrorKind::NotFound {
        fail!("{ERLANG_MISSING}")
    } else {
        fail!("cannot invoke Erlang/OTP: {error}")
    }
}

/// Writes one `.erl` file into `out_dir`, returning its path.
fn write_erl(out_dir: &Path, name: &str, source: &str) -> Result<PathBuf, Failure> {
    let path = out_dir.join(format!("{name}.erl"));
    fs::write(&path, source).map_err(|e| fail!("cannot write `{}`: {e}", path.display()))?;
    Ok(path)
}

/// Finds and validates the program's `fn main`. At most one module may
/// define it; when present it must take no parameters, return `()`, and
/// carry no residual `Tool<…>` effects (the boot module supplies an empty
/// handler map, so an unhandled tool call could only crash at runtime).
fn find_entry_point(modules: &[(PathBuf, IrModule)]) -> Result<Option<EntryPoint>, Failure> {
    let mut mains: Vec<(&PathBuf, &IrModule, &IrFnDef)> = Vec::new();
    for (path, module) in modules {
        for decl in &module.declarations {
            if let IrDecl::Fn(f) = decl
                && f.name == "main"
            {
                mains.push((path, module, f));
            }
        }
    }
    let (path, module, main) = match mains.as_slice() {
        [] => return Ok(None),
        [only] => *only,
        [(first, ..), (second, ..), ..] => {
            return Err(fail!(
                "multiple `fn main` definitions: `{}` and `{}`",
                first.display(),
                second.display()
            ));
        }
    };
    if !main.params.is_empty() {
        return Err(fail!(
            "`{}`: `fn main` takes no parameters in v0.1 (argument passthrough is reserved)",
            path.display()
        ));
    }
    if main.return_type != Type::tuple(Vec::new()) {
        return Err(fail!(
            "`{}`: `fn main` must return `()`, not `{}`",
            path.display(),
            main.return_type
        ));
    }
    let tools: Vec<String> = main
        .effect_row
        .effects()
        .filter(|e| e.head().as_str() == "Tool")
        .map(|e| e.to_string())
        .collect();
    if !tools.is_empty() {
        return Err(fail!(
            "`{}`: `fn main` has unhandled tool effects {{{}}}; wrap its body in a \
             `handle` block providing implementations",
            path.display(),
            tools.join(", ")
        ));
    }
    Ok(Some(EntryPoint {
        module: erlang_module_name(&module.name),
        takes_map: !main.effect_row.is_empty(),
    }))
}

/// Renders the boot module: starts the audit sink, registers every
/// signature table in `table_modules`, and calls `main` with an empty
/// handler map. Init arguments configure the run, so the module depends on
/// the program alone: `-hird_audit_file <path>` appends the audit stream
/// to that file instead of stdout, and `-hird_replay <path>` starts the
/// replay cursor over that log before `main` and requires it fully
/// consumed after. A crash in `main` still flushes the sink before
/// halting. Kept as generated source so the build output runs on plain
/// `erl` without the CLI.
fn boot_module(entry: &EntryPoint, table_modules: &[String]) -> String {
    let tables = table_modules
        .iter()
        .map(|module| format!("{module}:hird_tools@()"))
        .collect::<Vec<_>>();
    let mut out = String::new();
    let _ = writeln!(out, "%% Generated by the Hir\u{f0} compiler. Do not edit.");
    let _ = writeln!(out, "-module({BOOT_MODULE}).");
    let _ = writeln!(out, "-export([run/0, main/0]).");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "%% Starts the runtime, runs the program's main function, and flushes"
    );
    let _ = writeln!(
        out,
        "%% the audit sink. `-hird_audit_file Path` sends the audit stream to"
    );
    let _ = writeln!(
        out,
        "%% Path instead of stdout; `-hird_replay Log` replays tool calls from"
    );
    let _ = writeln!(out, "%% Log and requires it fully consumed.");
    let _ = writeln!(out, "main() ->");
    let _ = writeln!(out, "    Sink = case argument(hird_audit_file) of");
    let _ = writeln!(out, "        {{ok, Path}} -> {{file, Path}};");
    let _ = writeln!(out, "        none -> stdout");
    let _ = writeln!(out, "    end,");
    let _ = writeln!(
        out,
        "    {{ok, _}} = hird_audit:start_link([{{sink, Sink}}]),"
    );
    for table in &tables {
        let _ = writeln!(out, "    ok = hird_audit:register_tools({table}),");
    }
    let _ = writeln!(out, "    Replay = argument(hird_replay),");
    let _ = writeln!(out, "    case Replay of");
    let _ = writeln!(
        out,
        "        {{ok, Log}} -> {{ok, _}} = hird_replay:start_link(Log, [{}]);",
        tables.join(", ")
    );
    let _ = writeln!(out, "        none -> ok");
    let _ = writeln!(out, "    end,");
    let main_call = if entry.takes_map {
        format!("{}:main(#{{}})", entry.module)
    } else {
        format!("{}:main()", entry.module)
    };
    let _ = writeln!(out, "    Result = {main_call},");
    let _ = writeln!(out, "    ok = hird_audit:sync(),");
    let _ = writeln!(out, "    case Replay of");
    let _ = writeln!(out, "        {{ok, _}} -> ok = hird_replay:finish();");
    let _ = writeln!(out, "        none -> ok");
    let _ = writeln!(out, "    end,");
    let _ = writeln!(out, "    Result.");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "%% The value of init argument `-Flag Value`, given at most once."
    );
    let _ = writeln!(out, "argument(Flag) ->");
    let _ = writeln!(out, "    case init:get_argument(Flag) of");
    let _ = writeln!(out, "        {{ok, [[Value]]}} -> {{ok, Value}};");
    let _ = writeln!(out, "        error -> none");
    let _ = writeln!(out, "    end.");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "%% Entry point for `erl -noshell -s {BOOT_MODULE} run`: runs main and"
    );
    let _ = writeln!(
        out,
        "%% halts with a status reflecting success or crash, flushing the audit"
    );
    let _ = writeln!(out, "%% sink on the crash path too.");
    let _ = writeln!(out, "run() ->");
    let _ = writeln!(out, "    try main() of");
    let _ = writeln!(out, "        _ -> halt(0)");
    let _ = writeln!(out, "    catch");
    let _ = writeln!(out, "        Class:Reason:Stack ->");
    let _ = writeln!(
        out,
        "            io:format(standard_error, \"hird: runtime error: ~p~n\", [{{Class, Reason, Stack}}]),"
    );
    let _ = writeln!(out, "            _ = (catch hird_audit:sync()),");
    let _ = writeln!(out, "            halt(1)");
    let _ = writeln!(out, "    end.");
    out
}

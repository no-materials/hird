// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Erlang emission and BEAM compilation: write generated and runtime `.erl`
//! files into the build directory, generate the boot module, reuse `.beam`
//! files from the build cache, and drive `erl`, whose build driver compiles
//! the rest.

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

/// The build driver's module name: it compiles the modules a build did not
/// find in the build cache, in the emulator that goes on to run them.
const DRIVER_MODULE: &str = "hird_build";

/// The build driver's Erlang source. It fixes the compile options, so the
/// build cache keys every entry on it.
const DRIVER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../runtime/hird_build.erl"
));

/// The hand-written Erlang runtime library, embedded so the compiled binary
/// is self-contained.
const RUNTIME: [(&str, &str); 10] = [
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
    (DRIVER_MODULE, DRIVER),
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

/// Compiles and loads the build driver from the output directory, for a
/// build that did not find the driver itself in the build cache: an
/// `-eval` expression with no spaces or string literals, so no platform
/// quotes it. It ignores `ERL_COMPILER_OPTIONS`, so options that break a
/// compile are reported by the driver, module by module.
const BOOTSTRAP: &str = "{ok,[[D]]}=init:get_argument(hird_out),\
     F=filename:join(D,hird_build),\
     {ok,M,B}=compile:noenv_file(F,[binary,deterministic]),\
     {module,M}=code:load_binary(M,F,B)";

/// The advice printed when `erl` is not on `PATH`.
const ERLANG_MISSING: &str =
    "Erlang/OTP not found. Install from https://www.erlang.org/ or use asdf/nix.";

/// A compiled program's entry point: the base module defining `fn main`.
pub(crate) struct EntryPoint {
    /// The Erlang name of the module defining `main`.
    pub(crate) module: String,
    /// Whether `main` threads the handler map (non-empty effect row).
    pub(crate) takes_map: bool,
}

/// A written build directory: every module's `.erl`, and the `.beam` of
/// each the build cache held.
pub(crate) struct BuildOutput {
    /// The build output directory.
    pub(crate) out_dir: PathBuf,
    /// The program entry point, when some module defines `fn main`.
    pub(crate) entry: Option<EntryPoint>,
    /// The modules still to compile.
    pending: Pending,
    /// Modules copied from the build cache.
    reused: u64,
}

/// The modules a build did not find in the build cache, for the build
/// driver to compile. Paths are absolute, so none reads as an emulator
/// flag.
struct Pending {
    /// The output directory the driver compiles in.
    out_dir: PathBuf,
    /// The modules to compile.
    modules: Vec<String>,
    /// The cache directory and each module's entry key, when the cache is
    /// on.
    cache: Option<(PathBuf, Vec<String>)>,
}

/// Emits `modules` (paired with their source paths) plus the runtime and
/// boot module into `out_dir`, and copies in the `.beam` of every module
/// the build cache holds; [`compile`] or [`run`] compiles the rest.
/// Records the `emit`, `write`, and `reuse` phases and the module counters
/// in `timings`.
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

    let pending = timings.phase("reuse", || reuse(&sources, out_dir))?;
    let reused = (sources.len() - pending.modules.len()) as u64;
    timings.modules_compiled = pending.modules.len() as u64;
    timings.modules_reused = reused;
    Ok(BuildOutput {
        out_dir: out_dir.to_path_buf(),
        entry,
        pending,
        reused,
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

/// Copies the `.beam` of each of `sources` the build cache holds into
/// `out_dir`, returning the rest for the build driver.
fn reuse(sources: &[(&str, &str)], out_dir: &Path) -> Result<Pending, Failure> {
    let cache = BeamCache::open(DRIVER);
    let mut modules = Vec::new();
    let mut keys = Vec::new();
    for &(name, source) in sources {
        let Some(cache) = &cache else {
            modules.push(name.to_owned());
            continue;
        };
        let key = cache.key(source);
        if !cache.fetch(&key, &out_dir.join(format!("{name}.beam"))) {
            modules.push(name.to_owned());
            keys.push(key);
        }
    }
    let cache = match cache {
        Some(cache) => Some((absolute(cache.dir())?, keys)),
        None => None,
    };
    Ok(Pending {
        out_dir: absolute(out_dir)?,
        modules,
        cache,
    })
}

/// `path` made absolute.
fn absolute(path: &Path) -> Result<PathBuf, Failure> {
    std::path::absolute(path).map_err(|e| fail!("cannot resolve `{}`: {e}", path.display()))
}

/// Compiles the modules `build` did not find in the build cache: the build
/// driver compiles them in one emulator, in parallel, storing each in the
/// cache. Starts no emulator when nothing is pending. Records the `compile`
/// phase in `timings`.
pub(crate) fn compile(build: &BuildOutput, timings: &mut Timings) -> Result<(), Failure> {
    let pending = !build.pending.modules.is_empty();
    if pending {
        timings.emulator_boots += 1;
    }
    timings.phase("compile", || -> Result<(), Failure> {
        if !pending {
            return Ok(());
        }
        // The emulator starts as erlc starts its own: only the kernel
        // processes a compile needs, and the boot script that skips the
        // user's `.erlang`, spelled as erlc spells it (init expands `$ROOT`;
        // a bare name would be looked up in the working directory first).
        // The driver writes the compiler's reports to stderr; anything the
        // emulator prints to stdout belongs there too, as erlc's did.
        let status = Command::new("erl")
            .args(["+sbtu", "+A0", "-noinput", "-mode", "minimal"])
            .args(["-boot", "$ROOT/no_dot_erlang"])
            .arg("-pa")
            .arg(&build.out_dir)
            .args(driver_args(&build.pending, "build"))
            .stdin(Stdio::null())
            .stdout(io::stderr())
            .status()
            .map_err(erlang_unavailable)?;
        if !status.success() {
            return Err(fail!("compiling the generated Erlang failed with {status}"));
        }
        Ok(())
    })?;
    report_build(build);
    Ok(())
}

/// The init arguments that have the build driver compile `pending`, then
/// call its `function` (`build` halts, `run` runs the program). The driver
/// is compiled and loaded first when it is pending itself.
fn driver_args(pending: &Pending, function: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();
    if pending.modules.iter().any(|name| name == DRIVER_MODULE) {
        args.extend(["-eval", BOOTSTRAP].map(OsString::from));
    }
    args.extend(["-s", DRIVER_MODULE, function].map(OsString::from));
    args.extend([OsString::from("-hird_out"), pending.out_dir.clone().into()]);
    args.push("-hird_compile".into());
    args.extend(pending.modules.iter().map(OsString::from));
    if let Some((dir, keys)) = &pending.cache {
        args.extend([OsString::from("-hird_cache"), dir.clone().into()]);
        args.extend(keys.iter().map(OsString::from));
    }
    args
}

/// Prints what the build compiled and reused.
fn report_build(build: &BuildOutput) {
    let compiled = build.pending.modules.len();
    match build.reused {
        0 => eprintln!(
            "compiled {compiled} module(s) to {}",
            build.out_dir.display()
        ),
        reused => eprintln!(
            "compiled {compiled} module(s) to {}, reused {reused} from the build cache",
            build.out_dir.display()
        ),
    }
}

/// Runs a built program on BEAM through the generated boot module, returning
/// the emulator's exit status. Modules the build cache did not hold compile
/// first in the same emulator, through the build driver, so a run boots one
/// emulator whatever it compiles; a module that fails to compile exits it
/// with status 1 before the program starts.
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
    if build.entry.is_none() {
        return Err(fail!(
            "no `fn main` found: `hird run` needs a module defining `fn main() \u{2192} ()`"
        ));
    }
    report_build(build);
    timings.emulator_boots += 1;
    let status = timings.phase("run", || -> Result<_, Failure> {
        let mut erl = Command::new("erl");
        erl.arg("+Bi")
            .arg("-noshell")
            .arg("-pa")
            .arg(&build.out_dir);
        if build.pending.modules.is_empty() {
            erl.args(["-s", BOOT_MODULE, "run"]);
        } else {
            erl.args(driver_args(&build.pending, "run"));
        }
        let mut child = erl
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

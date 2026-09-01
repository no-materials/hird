// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Erlang emission and BEAM compilation: write generated and runtime `.erl`
//! files into the build directory, generate the boot module, and drive
//! `erlc`/`erl`.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::Mutex;

use hird_codegen::erlang_module_name;
use hird_ir::{IrDecl, IrFnDef, IrModule};
use hird_types::Type;

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
/// boot module into `out_dir`, then compiles everything with `erlc`. The
/// boot module's audit sink appends to `audit_file` when given, stdout
/// otherwise; with `replay` it starts a replay cursor over that log
/// before running `main`.
pub(crate) fn build(
    modules: &[(PathBuf, IrModule)],
    out_dir: &Path,
    audit_file: Option<&Path>,
    replay: Option<&Path>,
) -> Result<BuildOutput, Failure> {
    let entry = find_entry_point(modules)?;
    let audit_file = match audit_file {
        Some(path) => Some(
            path.to_str()
                .ok_or_else(|| fail!("audit file path `{}` is not valid UTF-8", path.display()))?,
        ),
        None => None,
    };
    let replay = match replay {
        Some(path) => Some(
            path.to_str()
                .ok_or_else(|| fail!("replay log path `{}` is not valid UTF-8", path.display()))?,
        ),
        None => None,
    };

    let mut emitted: Vec<(String, String)> = Vec::new();
    let mut origin: std::collections::BTreeMap<String, PathBuf> = RUNTIME
        .iter()
        .map(|(name, _)| ((*name).to_owned(), PathBuf::from("the Hirð runtime")))
        .collect();
    origin.insert(BOOT_MODULE.to_owned(), PathBuf::from("the Hirð runtime"));
    for (path, module) in modules {
        for out in hird_codegen::emit_modules(module, &path.display().to_string()) {
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

    fs::create_dir_all(out_dir).map_err(|e| fail!("cannot create `{}`: {e}", out_dir.display()))?;
    let mut erl_files = Vec::new();
    for (name, source) in emitted.iter().map(|(n, s)| (n.as_str(), s.as_str())) {
        erl_files.push(write_erl(out_dir, name, source)?);
    }
    for (name, source) in RUNTIME {
        erl_files.push(write_erl(out_dir, name, source)?);
    }
    if let Some(entry) = &entry {
        let tool_modules: Vec<String> = modules
            .iter()
            .filter(|(_, m)| m.declarations.iter().any(|d| matches!(d, IrDecl::Tool(_))))
            .map(|(_, m)| erlang_module_name(&m.name))
            .collect();
        erl_files.push(write_erl(
            out_dir,
            BOOT_MODULE,
            &boot_module(entry, &tool_modules, audit_file, replay),
        )?);
    }

    let mut erlc = Command::new("erlc");
    erlc.arg("-o").arg(out_dir).args(&erl_files);
    let output = erlc.output().map_err(erlang_unavailable)?;
    io::Write::write_all(&mut io::stderr(), &output.stdout)
        .and_then(|()| io::Write::write_all(&mut io::stderr(), &output.stderr))
        .map_err(|e| fail!("cannot write erlc output: {e}"))?;
    if !output.status.success() {
        return Err(fail!("erlc failed with {}", output.status));
    }

    eprintln!(
        "compiled {} module(s) to {}",
        erl_files.len(),
        out_dir.display()
    );
    Ok(BuildOutput {
        out_dir: out_dir.to_path_buf(),
        entry,
    })
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
pub(crate) fn run(build: &BuildOutput) -> Result<i32, Failure> {
    let entry_module = match &build.entry {
        Some(_) => BOOT_MODULE,
        None => {
            return Err(fail!(
                "no `fn main` found: `hird run` needs a module defining `fn main() \u{2192} ()`"
            ));
        }
    };
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
        .stdin(Stdio::piped())
        .spawn()
        .map_err(erlang_unavailable)?;
    let stdin = child.stdin.take().expect("the emulator's stdin is piped");
    stop_on_termination(stdin)?;
    let status = child
        .wait()
        .map_err(|e| fail!("cannot wait for Erlang/OTP: {e}"))?;
    Ok(status.code().unwrap_or(1))
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

/// Renders the boot module: starts the audit sink (appending to
/// `audit_file` when given, stdout otherwise), registers each
/// tool-declaring module's signature table, and calls `main` with an empty
/// handler map. With `replay` it also starts the replay cursor over that
/// log before `main` and requires the log fully consumed after. Kept as
/// generated source so the build output runs on plain `erl` without the
/// CLI.
fn boot_module(
    entry: &EntryPoint,
    tool_modules: &[String],
    audit_file: Option<&str>,
    replay: Option<&str>,
) -> String {
    let sink = match audit_file {
        Some(path) => format!("{{file, \"{}\"}}", erlang_string_escape(path)),
        None => "stdout".to_owned(),
    };
    let mut out = String::new();
    let _ = writeln!(out, "%% Generated by the Hir\u{f0} compiler. Do not edit.");
    let _ = writeln!(out, "-module({BOOT_MODULE}).");
    let _ = writeln!(out, "-export([run/0, main/0]).");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "%% Starts the runtime, runs the program's main function, and"
    );
    let _ = writeln!(out, "%% flushes the audit sink.");
    let _ = writeln!(out, "main() ->");
    let _ = writeln!(
        out,
        "    {{ok, _}} = hird_audit:start_link([{{sink, {sink}}}]),"
    );
    for module in tool_modules {
        let _ = writeln!(
            out,
            "    ok = hird_audit:register_tools({module}:hird_tools@()),"
        );
    }
    if let Some(log) = replay {
        let tables = tool_modules
            .iter()
            .map(|module| format!("{module}:hird_tools@()"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            out,
            "    {{ok, _}} = hird_replay:start_link(\"{}\", [{tables}]),",
            erlang_string_escape(log)
        );
    }
    let main_call = if entry.takes_map {
        format!("{}:main(#{{}})", entry.module)
    } else {
        format!("{}:main()", entry.module)
    };
    let _ = writeln!(out, "    Result = {main_call},");
    let _ = writeln!(out, "    ok = hird_audit:sync(),");
    if replay.is_some() {
        let _ = writeln!(out, "    ok = hird_replay:finish(),");
    }
    let _ = writeln!(out, "    Result.");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "%% Entry point for `erl -noshell -s {BOOT_MODULE} run`: runs main and"
    );
    let _ = writeln!(out, "%% halts with a status reflecting success or crash.");
    let _ = writeln!(out, "run() ->");
    let _ = writeln!(out, "    try main() of");
    let _ = writeln!(out, "        _ -> halt(0)");
    let _ = writeln!(out, "    catch");
    let _ = writeln!(out, "        Class:Reason:Stack ->");
    let _ = writeln!(
        out,
        "            io:format(standard_error, \"hird: runtime error: ~p~n\", [{{Class, Reason, Stack}}]),"
    );
    let _ = writeln!(out, "            halt(1)");
    let _ = writeln!(out, "    end.");
    out
}

/// Escapes `\` and `"` for embedding in an Erlang string literal.
fn erlang_string_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Whole-program (multi-module) checking: import resolution, qualified names,
//! visibility, circular imports, opaque-type discipline, and per-namespace
//! duplicate detection.

use std::fmt::Write;

use hird_ast::{AstNode, SourceFile};
use hird_check::{CheckedProgram, ModuleName, Severity, check_program};

/// Parses each `(module name, source)` pair, checks the program, and renders
/// every module's resolved bindings followed by its diagnostics, in
/// module-name order.
fn check_modules(modules: &[(&str, &str)]) -> String {
    let parsed: Vec<_> = modules
        .iter()
        .map(|(_, src)| hird_parse::parse(src, 0))
        .collect();
    for ((name, src), p) in modules.iter().zip(&parsed) {
        assert!(
            p.is_ok(),
            "module `{name}` has parse errors in `{src}`: {:?}",
            p.diagnostics()
        );
    }
    let files: Vec<(ModuleName, SourceFile)> = modules
        .iter()
        .zip(&parsed)
        .map(|((name, _), p)| {
            (
                ModuleName::new(*name),
                SourceFile::cast(p.syntax().clone()).expect("root is a source file"),
            )
        })
        .collect();
    render(&check_program(&files))
}

/// Parses each `(module name, source)` pair and checks the program, returning
/// the raw result for structural assertions.
fn checked(modules: &[(&str, &str)]) -> CheckedProgram {
    let files: Vec<(ModuleName, SourceFile)> = modules
        .iter()
        .map(|(name, src)| {
            let parsed = hird_parse::parse(src, 0);
            assert!(
                parsed.is_ok(),
                "module `{name}` has parse errors: {:?}",
                parsed.diagnostics()
            );
            (
                ModuleName::new(*name),
                SourceFile::cast(parsed.syntax().clone()).expect("root is a source file"),
            )
        })
        .collect();
    check_program(&files)
}

/// Renders a checked program: a header per module, then its bindings and
/// diagnostics (secondary spans indented beneath their diagnostic).
fn render(program: &CheckedProgram) -> String {
    let mut out = String::new();
    for (name, checked) in &program.modules {
        writeln!(out, "== {name} ==").unwrap();
        for (binding, ty) in &checked.bindings {
            writeln!(out, "{binding} : {}", ty.normalized()).unwrap();
        }
        for diag in &checked.diagnostics {
            let severity = match diag.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            writeln!(
                out,
                "{severity}[{:?}] {}..{}: {}",
                diag.code, diag.span.start, diag.span.end, diag.message
            )
            .unwrap();
            for rel in &diag.related {
                writeln!(
                    out,
                    "  related {}..{}: {}",
                    rel.span.start, rel.span.end, rel.message
                )
                .unwrap();
            }
        }
    }
    out
}

// ── import resolution: selective, aliased, wildcard ─────────────

#[test]
fn selective_import_resolves() {
    insta::assert_snapshot!(check_modules(&[
        ("Ets", "module Ets\npub fn lookup(k: Int) -> Int = k"),
        (
            "App",
            "module App\nuse Ets.{lookup}\npub fn run(x: Int) -> Int = lookup(x)",
        ),
    ]));
}

#[test]
fn aliased_import_qualified_call() {
    insta::assert_snapshot!(check_modules(&[
        ("Log", "module Log\npub fn info(msg: String) -> Int = 0"),
        (
            "App",
            "module App\nuse Log as L\npub fn run() -> Int = L.info(\"hi\")",
        ),
    ]));
}

#[test]
fn wildcard_import_qualified_call() {
    insta::assert_snapshot!(check_modules(&[
        ("Ets", "module Ets\npub fn lookup(k: Int) -> Int = k"),
        (
            "App",
            "module App\nuse Ets\npub fn run(x: Int) -> Int = Ets.lookup(x)",
        ),
    ]));
}

#[test]
fn transparent_constructors_importable() {
    insta::assert_snapshot!(check_modules(&[
        ("Opt", "module Opt\npub type Maybe = Just(Int) | Nada"),
        (
            "App",
            "module App\n\
             use Opt.{Maybe, Just, Nada}\n\
             pub fn unwrap(m: Maybe) -> Int = match m { Just(n) -> n, Nada -> 0, }\n\
             pub fn wrap(n: Int) -> Maybe = Just(n)",
        ),
    ]));
}

// ── visibility & qualified-name failures ────────────────────────

#[test]
fn private_name_is_not_exported() {
    insta::assert_snapshot!(check_modules(&[
        ("M", "module M\nfn secret(x: Int) -> Int = x"),
        (
            "App",
            "module App\nuse M.{secret}\npub fn run(x: Int) -> Int = x"
        ),
    ]));
}

#[test]
fn qualified_name_unknown_member() {
    insta::assert_snapshot!(check_modules(&[
        ("Ets", "module Ets\npub fn lookup(k: Int) -> Int = k"),
        (
            "App",
            "module App\nuse Ets\npub fn run(x: Int) -> Int = Ets.nope(x)",
        ),
    ]));
}

#[test]
fn unresolved_module() {
    insta::assert_snapshot!(check_modules(&[(
        "App",
        "module App\nuse Nowhere.{thing}\npub fn run() -> Int = 0",
    )]));
}

// ── circular imports ────────────────────────────────────────────

#[test]
fn circular_import_is_rejected() {
    insta::assert_snapshot!(check_modules(&[
        ("A", "module A\nuse B\npub fn fa() -> Int = 0"),
        ("B", "module B\nuse A\npub fn fb() -> Int = 0"),
    ]));
}

// ── opaque types ────────────────────────────────────────────────

#[test]
fn opaque_capability_used_legitimately() {
    insta::assert_snapshot!(check_modules(&[
        (
            "Ets",
            "module Ets\n\
             pub opaque type Table = MkTable(Int)\n\
             pub fn empty() -> Table = MkTable(0)\n\
             pub fn size(t: Table) -> Int = match t { MkTable(n) -> n, }",
        ),
        (
            "App",
            "module App\n\
             use Ets.{Table, empty, size}\n\
             pub fn use_table() -> Int = size(empty())\n\
             pub fn store(t: Table) -> List<Table> = [t]",
        ),
    ]));
}

#[test]
fn opaque_destructure_outside_module_errors() {
    insta::assert_snapshot!(check_modules(&[
        ("Ets", "module Ets\npub opaque type Table = MkTable(Int)"),
        (
            "App",
            "module App\nuse Ets.{Table}\npub fn peek(t: Table) -> Int = match t { MkTable(n) -> n, }",
        ),
    ]));
}

#[test]
fn opaque_construct_outside_module_errors() {
    insta::assert_snapshot!(check_modules(&[
        ("Ets", "module Ets\npub opaque type Table = MkTable(Int)"),
        (
            "App",
            "module App\nuse Ets.{Table}\npub fn forge() -> Table = MkTable(0)"
        ),
    ]));
}

// ── duplicate / collision detection (two namespaces) ────────────

#[test]
fn duplicate_value_definition() {
    insta::assert_snapshot!(check_modules(&[(
        "M",
        "module M\nfn foo() -> Int = 1\nfn foo() -> Int = 2",
    )]));
}

#[test]
fn duplicate_type_definition() {
    insta::assert_snapshot!(check_modules(&[("M", "module M\ntype T = A\ntype T = B")]));
}

#[test]
fn type_and_constructor_share_a_name() {
    insta::assert_snapshot!(check_modules(&[(
        "M",
        "module M\npub type Email = Email(String)\npub fn make(s: String) -> Email = Email(s)",
    )]));
}

#[test]
fn import_collides_with_local_definition() {
    insta::assert_snapshot!(check_modules(&[
        ("M", "module M\npub fn helper(x: Int) -> Int = x"),
        (
            "App",
            "module App\nuse M.{helper}\nfn helper(x: Int) -> Int = x"
        ),
    ]));
}

#[test]
fn import_collides_with_import() {
    insta::assert_snapshot!(check_modules(&[
        ("A", "module A\npub fn shared() -> Int = 1"),
        ("B", "module B\npub fn shared() -> Int = 2"),
        (
            "App",
            "module App\nuse A.{shared}\nuse B.{shared}\npub fn run() -> Int = shared()",
        ),
    ]));
}

// ── import origins for lowering ─────────────────────────────────

/// Each unshadowed use of an unqualified imported function records its
/// defining module (call and value positions alike); a shadowed use records
/// nothing, and the defining module records nothing for its own functions.
#[test]
fn import_origins_recorded_for_unshadowed_uses() {
    let program = checked(&[
        ("Lib", "module Lib\npub fn double(x: Int) -> Int = x + x"),
        (
            "App",
            "module App\nuse Lib.{double}\n\
             pub fn call(x: Int) -> Int = double(x)\n\
             pub fn value(x: Int) -> Int = let f = double in f(x)\n\
             pub fn shadowed(x: Int) -> Int = let double = \\y -> y in double(x)",
        ),
    ]);
    let app = &program.modules[&ModuleName::new("App")];
    assert!(
        app.diagnostics
            .iter()
            .all(|d| d.severity == Severity::Warning),
        "diags: {:?}",
        app.diagnostics
    );
    let origins: Vec<&str> = app
        .import_origins
        .values()
        .map(ModuleName::as_str)
        .collect();
    assert_eq!(origins, ["Lib", "Lib"], "one origin per unshadowed use");
    let lib = &program.modules[&ModuleName::new("Lib")];
    assert!(
        lib.import_origins.is_empty(),
        "origins: {:?}",
        lib.import_origins
    );
}

// ── module-name validation ──────────────────────────────────────

#[test]
fn module_name_must_match_path() {
    insta::assert_snapshot!(check_modules(&[(
        "Right",
        "module Wrong\npub fn f() -> Int = 0"
    )]));
}

// ── tools, actors, and supervisors across modules ───────────────

/// A worker module exporting a tool, an actor with its mailbox, and a
/// supervisor over that actor.
const WORKER: &str = "module Worker\n\
     pub tool Run : { job: String } -> ()\n\
     pub actor Runner {\n\
       state: Int,\n\
       message: WorkerMsg = | Do(String) | Halt,\n\
       init: fn(n: Int) ! {} = n,\n\
       handle Do(s), n ! {Tool<Run>} = match run({ job: s }) { _ -> Continue(n + 1) },\n\
       handle Halt, _ ! {} = Stop,\n\
     } ! {Tool<Run>}\n\
     pub supervisor RunnerSup {\n\
       strategy: one_for_one,\n\
       intensity: 3,\n\
       period: 60,\n\
       children: [\n\
         { id: runner, actor: Runner, start_args: 0, restart: permanent },\n\
       ]\n\
     }";

#[test]
fn imported_tool_is_callable_handleable_and_nameable_in_rows() {
    insta::assert_snapshot!(check_modules(&[
        ("Worker", WORKER),
        (
            "App",
            "module App\n\
             use Worker.{Run}\n\
             pub fn go(j: String) -> () ! {Tool<Run>} = run({ job: j })\n\
             pub fn dry(j: String) -> () = handle { Tool<Run> -> \\a -> () } in go(j)",
        ),
    ]));
}

#[test]
fn imported_tool_handler_must_match_its_signature() {
    insta::assert_snapshot!(check_modules(&[
        ("Worker", WORKER),
        (
            "App",
            "module App\n\
             use Worker.{Run}\n\
             pub fn dry(j: String) -> () = handle { Tool<Run> -> \\a -> 42 } in run({ job: j })",
        ),
    ]));
}

#[test]
fn imported_tool_is_callable_qualified() {
    insta::assert_snapshot!(check_modules(&[
        ("Worker", WORKER),
        (
            "App",
            "module App\n\
             use Worker.{Run}\n\
             use Worker\n\
             use Worker as W\n\
             pub fn go(j: String) -> () ! {Tool<Run>} = Worker.run({ job: j })\n\
             pub fn again(j: String) -> () ! {Tool<Run>} = W.run({ job: j })",
        ),
    ]));
}

#[test]
fn imported_tools_are_recorded_by_qualified_spelling() {
    let program = checked(&[
        ("Worker", WORKER),
        (
            "App",
            "module App\n\
             use Worker.{Run}\n\
             use Worker as W\n\
             pub fn go(j: String) -> () ! {Tool<Run>} = run({ job: j })\n\
             pub fn again(j: String) -> () ! {Tool<Run>} = W.run({ job: j })",
        ),
    ]);
    assert!(!program.has_errors(), "{program:?}");
    let app = &program.modules[&ModuleName::new("App")];
    let spellings: Vec<&str> = app.imported_tools.keys().map(String::as_str).collect();
    assert_eq!(spellings, ["W.run", "Worker.run"]);
    for tool in app.imported_tools.values() {
        assert_eq!(tool.module.as_str(), "Worker");
        assert_eq!(tool.name.as_str(), "Run");
    }
    let worker = &program.modules[&ModuleName::new("Worker")];
    assert!(worker.imported_tools.is_empty());
    // The declaring module's own table stays its own.
    assert_eq!(worker.tools.len(), 1);
    assert!(app.tools.is_empty());
}

#[test]
fn imported_actor_spawns_and_its_mailbox_is_nameable() {
    insta::assert_snapshot!(check_modules(&[
        ("Worker", WORKER),
        (
            "App",
            "module App\n\
             use Worker.{Runner, WorkerMsg, Do}\n\
             use Worker\n\
             pub fn start() -> Pid<WorkerMsg> ! {Spawn<WorkerMsg>} = spawn(Runner, 0)\n\
             pub fn kick(p: Pid<WorkerMsg>, j: String) -> () ! {Send<WorkerMsg>} = send(p, Do(j))\n\
             pub fn halt(p: Pid<WorkerMsg>) -> () ! {Send<WorkerMsg>} = send(p, Worker.Halt)",
        ),
    ]));
}

#[test]
fn imported_actor_is_not_a_value() {
    insta::assert_snapshot!(check_modules(&[
        ("Worker", WORKER),
        (
            "App",
            "module App\nuse Worker.{Runner}\npub fn bad() -> Int = Runner",
        ),
    ]));
}

#[test]
fn imported_supervisor_supervises_and_looks_up_children() {
    insta::assert_snapshot!(check_modules(&[
        ("Worker", WORKER),
        (
            "App",
            "module App\n\
             use Worker.{RunnerSup, WorkerMsg}\n\
             pub fn boot() -> () ! {Supervise} = supervise(RunnerSup)\n\
             pub fn runner() -> Pid<WorkerMsg> = child(RunnerSup, runner)\n\
             pub fn missing() -> Pid<WorkerMsg> = child(RunnerSup, nobody)",
        ),
    ]));
}

#[test]
fn private_tool_actor_and_supervisor_are_not_exported() {
    insta::assert_snapshot!(check_modules(&[
        (
            "Worker",
            "module Worker\n\
             tool Run : { job: String } -> ()\n\
             actor Runner {\n\
               state: Int,\n\
               message: WorkerMsg = | Do(String),\n\
               init: fn(n: Int) ! {} = n,\n\
               handle Do(s), n ! {} = Continue(n),\n\
             } ! {}\n\
             supervisor RunnerSup {\n\
               strategy: one_for_one,\n\
               intensity: 3,\n\
               period: 60,\n\
               children: [\n\
                 { id: runner, actor: Runner, start_args: 0, restart: permanent },\n\
               ]\n\
             }",
        ),
        (
            "App",
            "module App\n\
             use Worker.{Run, Runner, WorkerMsg, Do, RunnerSup}\n\
             use Worker\n\
             pub fn go(j: String) = Worker.run({ job: j })",
        ),
    ]));
}

#[test]
fn imported_actor_can_be_a_supervised_child() {
    insta::assert_snapshot!(check_modules(&[
        ("Worker", WORKER),
        (
            "App",
            "module App\n\
             use Worker.{Runner, WorkerMsg}\n\
             supervisor AppSup {\n\
               strategy: one_for_one,\n\
               intensity: 1,\n\
               period: 5,\n\
               children: [\n\
                 { id: worker, actor: Runner, start_args: 1, restart: transient },\n\
               ]\n\
             }\n\
             pub fn worker() -> Pid<WorkerMsg> = child(AppSup, worker)",
        ),
    ]));
}

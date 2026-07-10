// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Erlang emission coverage: typed programs through the full pipeline
//! (parse → check → lower → emit), snapshotted as generated Erlang and — when
//! `erlc` is on the `PATH` — compiled with stock `erlc`.

use hird_ast::{AstNode, SourceFile};
use hird_codegen::{EmittedModule, emit_modules};
use hird_ir::IrModule;

/// Every fixture: `(module name, Hirð source)`. The erlc validation test
/// compiles all of them, so each snapshot is also a compile guarantee.
const PROGRAMS: &[(&str, &str)] = &[
    ("Math", MATH),
    ("Pick", PICK),
    ("Opt", OPT),
    ("Build", BUILD),
    ("Shadow", SHADOW),
    ("Funs", FUNS),
    ("Person", PERSON),
    ("Values", VALUES),
    ("Repo", REPO),
    ("Audited", AUDITED),
    ("Multi", MULTI),
    ("Eta", ETA),
    ("Boot", BOOT),
    ("Msg", MSG),
    ("Boom", BOOM),
    ("Ffi", FFI),
    ("Reserved", RESERVED),
    ("Workshop", WORKER),
    ("Clock", CLOCK),
    ("Duo", DUO),
    ("Echo", ECHO),
];

const MATH: &str = "fn add(x: Int, y: Int) -> Int = x + y\n\
     fn precedence(a: Int, b: Int) -> Bool = (a + b) * 2 - 1 == b / 2\n\
     fn logic(p: Bool, q: Bool) -> Bool = p && q || p != q";

const PICK: &str = "fn pick(b: Bool) -> Int = if b then 1 else 2";

const OPT: &str = "type Option<a> = Some(a) | None\n\
     fn unwrap(opt: Option<Int>) -> Int = match opt { Some(x) -> x, None -> 0, }";

const BUILD: &str = "type List2<a> = Cons(a, List2<a>) | Nil\n\
     fn build() -> List2<Int> = Cons(1, Cons(2, Nil))";

const SHADOW: &str = "fn chain() -> Int = let x = 1 in let x = x + 1 in let y = x in x + y";

const FUNS: &str = "fn use_id() -> Int = let id = \\x -> x in id(1)\n\
     fn answer() -> Int = 42\n\
     fn get() = answer\n\
     fn immediate() -> Int = (\\x y -> x)(1, 2)";

const PERSON: &str = "fn make() = { name: \"x\", age: 1 }\n\
     fn age() -> Int = let r = { name: \"x\", age: 1 } in r.age";

const VALUES: &str = "fn triple() -> (Int, String, Bool) = (1, \"a\", True)\n\
     fn nums() -> List<Int> = [1, 2, 3]\n\
     fn nothing() = ()\n\
     fn pi() -> Float = 3.14";

const REPO: &str = "effect Tool<t>\n\
     type Path = Path(String)\n\
     type St = St(Int)\n\
     tool ReadRepo : { path: Path } -> St\n\
     fn scan(p: Path) -> St ! {Tool<ReadRepo>} = read_repo({ path: p })";

const AUDITED: &str = "effect Log\n\
     effect Tool<t>\n\
     tool Repo : { x: Int } -> Int\n\
     fn audited(f: Int -> Int ! {Tool<Repo>}, logh: { x: Int } -> Int ! {Log}) -> Int ! {Log} =\n\
       handle { Tool<Repo> -> logh } in f(0)";

const MULTI: &str = "effect Log\n\
     effect Tool<t>\n\
     tool Repo : { x: Int } -> Int\n\
     fn run(f: Int -> Int ! {Log, Tool<Repo>}, lh: Int -> Int, th: { x: Int } -> Int) -> Int =\n\
       handle { Log -> lh, Tool<Repo> -> th } in f(0)";

const ETA: &str = "fn apply(g: Int -> Int ! {r}, x: Int) -> Int ! {r} = g(x)\n\
     fn call_pure(f: Int -> Int, x: Int) -> Int = f(x)\n\
     fn inc(n: Int) -> Int = n + 1\n\
     fn absorb() -> Int = apply(inc, 1)\n\
     fn supply() -> Int = let id = \\x -> x in call_pure(id, 2)";

const BOOT: &str = "effect Spawn<t>\n\
     type St = St(Int)\n\
     actor Counter {\n\
       state: St,\n\
       message: Msg = | Inc,\n\
       init: fn(s: St) -> St ! {} = s,\n\
       handle Inc, St(n) -> St ! {} = St(n + 1),\n\
     }\n\
     fn boot(s: St) -> Pid<Msg> ! {Spawn<Msg>} = spawn(Counter, s)";

const MSG: &str = "effect Send<t>\n\
     effect Await<t>\n\
     type Status = Status(Int)\n\
     type St = St(Int)\n\
     actor Counter {\n\
       state: St,\n\
       message: Msg = | Inc | Get(ReplyTo<Status>),\n\
       init: fn(s: St) -> St ! {} = s,\n\
       handle Inc, St(n) -> St ! {} = St(n + 1),\n\
       handle Get(r), St(n) -> St ! {Send<Status>} = let sent = reply(r, Status(n)) in St(n),\n\
     } ! {Send<Status>}\n\
     fn poke(p: Pid<Msg>) ! {Send<Msg>} = send(p, Inc)\n\
     fn query(p: Pid<Msg>) -> Status ! {Send<Msg>, Await<Status>} = request(p, Get)";

const BOOM: &str = "type Option<a> = Some(a) | None\n\
     fn unwrap(o: Option<Int>) -> Int = match o { Some(x) -> x, None -> crash!(\"empty\"), }";

const FFI: &str = "extern fn sqrt(x: Float) -> Float\n\
     fn twice(x: Float) -> Float = sqrt(sqrt(x))";

const RESERVED: &str = "type Marker = End | Query\n\
     fn rem(m: Marker) -> Marker = match m { End -> Query, Query -> End, }";

const WORKER: &str = "effect Tool<t>\n\
     type St = St(Int)\n\
     tool Audit : { n: Int } -> Int\n\
     actor Worker {\n\
       state: St,\n\
       message: Msg = | Set(Int) | Bump,\n\
       init: fn(s: St) -> St ! {} = s,\n\
       handle Set(x), St(_) -> St ! {} = St(x),\n\
       handle Bump, St(n) -> St ! {Tool<Audit>} = St(audit({ n: n })),\n\
     } ! {Tool<Audit>}";

const CLOCK: &str = "type St = St(Int)\n\
     fn bump(n: Int) -> Int = n + 1\n\
     actor Ticker {\n\
       state: St,\n\
       message: Msg = | Tick | Via,\n\
       init: fn(s: St) -> St ! {} = s,\n\
       handle Tick, St(n) -> St ! {} = St(bump(n)),\n\
       handle Via, St(n) -> St ! {} = let f = bump in St(f(n)),\n\
     }";

const DUO: &str = "effect Spawn<t>\n\
     type St = St(Int)\n\
     actor Pair {\n\
       state: St,\n\
       message: Msg = | Nop,\n\
       init: fn(a: Int, b: Int) -> St ! {} = St(a + b),\n\
       handle Nop, St(n) -> St ! {} = St(n),\n\
     }\n\
     fn boot() -> Pid<Msg> ! {Spawn<Msg>} = spawn(Pair, 1, 2)";

const ECHO: &str = "effect Send<t>\n\
     type Ping = Ping\n\
     type St = St(Int)\n\
     actor Responder {\n\
       state: St,\n\
       message: Msg = | Get(ReplyTo<Ping>),\n\
       init: fn(s: St) -> St ! {} = s,\n\
       handle Get(r), St(n) -> St ! {Send<Ping>} = let ack = reply(r, Ping) in St(n),\n\
     } ! {Send<Ping>}";

/// Parses, checks, and lowers `source`, panicking on any parse or type error.
fn lower(source: &str, name: &str) -> IrModule {
    let parsed = hird_parse::parse(source, 0);
    assert!(
        parsed.is_ok(),
        "test source has parse errors: {:?}",
        parsed.diagnostics()
    );
    let file = SourceFile::cast(parsed.syntax().clone()).expect("root is a source file");
    let checked = hird_check::check(&file, 0);
    assert!(
        !checked.has_errors(),
        "test source has type errors: {:?}",
        checked.diagnostics
    );
    hird_ir::lower_module(&file, &checked, name)
}

/// Emits `source` as Erlang modules, with a `src/<module>.hird` source path.
fn emit_all(source: &str, name: &str) -> Vec<EmittedModule> {
    let module = lower(source, name);
    emit_modules(&module, &format!("src/{}.hird", name.to_lowercase()))
}

/// The base module's source for `source`.
fn emit(source: &str, name: &str) -> String {
    emit_all(source, name).swap_remove(0).source
}

/// The emitted module named `module_name` (an actor's `gen_server` module).
fn actor_module(source: &str, name: &str, module_name: &str) -> String {
    emit_all(source, name)
        .into_iter()
        .find(|m| m.name == module_name)
        .expect("actor module is emitted")
        .source
}

/// Every emitted module joined, each headed by its target file name.
fn emit_joined(source: &str, name: &str) -> String {
    let sections: Vec<String> = emit_all(source, name)
        .into_iter()
        .map(|m| format!("%%%% {}.erl\n{}", m.name, m.source))
        .collect();
    sections.join("\n")
}

/// The fixture source registered under `name`.
fn program(name: &str) -> &'static str {
    PROGRAMS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, src)| *src)
        .expect("fixture is registered")
}

// ── snapshots ────────────────────────────────────────────────────

#[test]
fn snapshot_pure_functions_and_operators() {
    insta::assert_snapshot!(emit(program("Math"), "Math"));
}

#[test]
fn snapshot_if_desugars_to_case() {
    insta::assert_snapshot!(emit(program("Pick"), "Pick"));
}

#[test]
fn snapshot_adt_match_and_constructors() {
    insta::assert_snapshot!(emit(program("Opt"), "Opt"));
}

#[test]
fn snapshot_nested_constructors() {
    insta::assert_snapshot!(emit(program("Build"), "Build"));
}

#[test]
fn snapshot_let_shadowing_freshens_variables() {
    insta::assert_snapshot!(emit(program("Shadow"), "Shadow"));
}

#[test]
fn snapshot_lambdas_and_function_values() {
    insta::assert_snapshot!(emit(program("Funs"), "Funs"));
}

#[test]
fn snapshot_records_and_field_access() {
    insta::assert_snapshot!(emit(program("Person"), "Person"));
}

#[test]
fn snapshot_tuples_lists_unit_and_literals() {
    insta::assert_snapshot!(emit(program("Values"), "Values"));
}

#[test]
fn snapshot_effectful_function_and_tool_dispatch() {
    insta::assert_snapshot!(emit(program("Repo"), "Repo"));
}

#[test]
fn snapshot_handle_block_extends_handler_map() {
    insta::assert_snapshot!(emit(program("Audited"), "Audited"));
}

#[test]
fn snapshot_handle_multi_arm() {
    insta::assert_snapshot!(emit(program("Multi"), "Multi"));
}

#[test]
fn snapshot_eta_expansion_of_pure_argument() {
    insta::assert_snapshot!(emit(program("Eta"), "Eta"));
}

#[test]
fn snapshot_spawn() {
    insta::assert_snapshot!(emit(program("Boot"), "Boot"));
}

#[test]
fn snapshot_send_and_request() {
    insta::assert_snapshot!(emit(program("Msg"), "Msg"));
}

#[test]
fn snapshot_crash_in_match_arm() {
    insta::assert_snapshot!(emit(program("Boom"), "Boom"));
}

#[test]
fn snapshot_extern_stub() {
    insta::assert_snapshot!(emit(program("Ffi"), "Ffi"));
}

#[test]
fn snapshot_reserved_words_are_quoted() {
    insta::assert_snapshot!(emit(program("Reserved"), "Reserved"));
}

#[test]
fn snapshot_actor_gen_server_cast_only() {
    insta::assert_snapshot!(actor_module(program("Boot"), "Boot", "hird_counter"));
}

#[test]
fn snapshot_actor_gen_server_call_and_reply() {
    insta::assert_snapshot!(actor_module(program("Msg"), "Msg", "hird_counter"));
}

#[test]
fn snapshot_actor_payload_and_tool_dispatch() {
    insta::assert_snapshot!(actor_module(program("Workshop"), "Workshop", "hird_worker"));
}

#[test]
fn snapshot_actor_qualifies_module_functions() {
    insta::assert_snapshot!(actor_module(program("Clock"), "Clock", "hird_ticker"));
}

#[test]
fn snapshot_actor_multi_param_init() {
    insta::assert_snapshot!(emit_joined(program("Duo"), "Duo"));
}

#[test]
fn snapshot_actor_call_only_cast_fallback() {
    insta::assert_snapshot!(actor_module(program("Echo"), "Echo", "hird_responder"));
}

// ── erlc validation ──────────────────────────────────────────────

/// Compiles every fixture's generated Erlang with stock `erlc`. Skipped (with
/// a note) when `erlc` is not on the `PATH`.
#[test]
fn generated_erlang_compiles_with_erlc() {
    let erlc = std::process::Command::new("erlc").arg("-version").output();
    if erlc.is_err() {
        eprintln!("skipping: erlc not found on PATH");
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("erlc");
    std::fs::create_dir_all(&dir).expect("create erlc scratch dir");
    for (name, source) in PROGRAMS {
        for module in emit_all(source, name) {
            let path = dir.join(format!("{}.erl", module.name));
            std::fs::write(&path, &module.source).expect("write generated module");
            let output = std::process::Command::new("erlc")
                .arg("-o")
                .arg(&dir)
                .arg(&path)
                .output()
                .expect("run erlc");
            assert!(
                output.status.success(),
                "erlc rejected {}:\n{}{}\n--- generated ---\n{}",
                module.name,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
                module.source,
            );
        }
    }
}

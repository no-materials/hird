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
    ("Deploy", DEPLOY),
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
    ("Solo", SOLO),
    ("Tree", TREE),
    ("Fleet", FLEET),
    ("Nest", NEST),
    ("Drift", DRIFT),
    ("Idle", IDLE),
    ("Wire", WIRE),
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

const DEPLOY: &str = "effect Log\n\
     effect Tool<t>\n\
     type Path = Path(String)\n\
     type St = St(Int)\n\
     tool ReadRepo : { path: Path } -> St\n\
     fn mock(args: { path: Path }) -> St = St(0)\n\
     fn unit_log(msg: String) -> St = St(1)\n\
     fn demo(run: Int -> Int ! {Tool<ReadRepo>}) -> Int ! {Install, Tool<ReadRepo>} =\n\
       install { Tool<ReadRepo> -> mock, Log -> unit_log } in run(0)";

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

const SOLO: &str = "type St = St(Int)\n\
     fn default_config() -> St = St(0)\n\
     actor Planner {\n\
       state: St,\n\
       message: PlannerMsg = | Nop,\n\
       init: fn(c: St) -> St ! {} = c,\n\
       handle Nop, st -> St ! {} = st,\n\
     }\n\
     supervisor PlannerSup {\n\
       strategy: one_for_one,\n\
       intensity: 5,\n\
       period: 60,\n\
       children: [\n\
         { id: planner, actor: Planner, start_args: default_config(), restart: permanent },\n\
       ]\n\
     }";

const TREE: &str = "effect Send<t>\n\
     type St = St(Int)\n\
     fn default_config() -> St = St(0)\n\
     actor Planner {\n\
       state: St,\n\
       message: PlannerMsg = | Nop,\n\
       init: fn(c: St) -> St ! {} = c,\n\
       handle Nop, st -> St ! {} = st,\n\
     }\n\
     supervisor PlannerSup {\n\
       strategy: one_for_one,\n\
       intensity: 5,\n\
       period: 60,\n\
       children: [\n\
         { id: planner, actor: Planner, start_args: default_config(), restart: permanent },\n\
       ]\n\
     }\n\
     fn boot() ! {Supervise, Send<PlannerMsg>} =\n\
       let u = supervise(PlannerSup) in\n\
       send(child(PlannerSup, planner), Nop)\n\
     fn serve() ! {Supervise, Stand} =\n\
       let u = supervise(PlannerSup) in\n\
       stand()";

const FLEET: &str = "type St = St(Int)\n\
     fn planner_config() -> St = St(0)\n\
     actor Planner {\n\
       state: St,\n\
       message: PlannerMsg = | Plan,\n\
       init: fn(c: St) -> St ! {} = c,\n\
       handle Plan, st -> St ! {} = st,\n\
     }\n\
     actor Worker {\n\
       state: St,\n\
       message: WorkerMsg = | Work,\n\
       init: fn(c: St) -> St ! {} = c,\n\
       handle Work, st -> St ! {} = st,\n\
     }\n\
     actor Logger {\n\
       state: St,\n\
       message: LoggerMsg = | Note,\n\
       init: fn(c: St) -> St ! {} = c,\n\
       handle Note, st -> St ! {} = st,\n\
     }\n\
     supervisor RootSup {\n\
       strategy: one_for_one,\n\
       intensity: 3,\n\
       period: 10,\n\
       children: [\n\
         { id: planner, actor: Planner, start_args: planner_config(), restart: permanent },\n\
         { id: worker, actor: Worker, start_args: St(1), restart: transient },\n\
         { id: logger, actor: Logger, start_args: St(2), restart: temporary },\n\
       ]\n\
     }";

const NEST: &str = "type St = St(Int)\n\
     actor Planner {\n\
       state: St,\n\
       message: PlannerMsg = | Nop,\n\
       init: fn(c: St) -> St ! {} = c,\n\
       handle Nop, st -> St ! {} = st,\n\
     }\n\
     actor Worker {\n\
       state: St,\n\
       message: WorkerMsg = | Nap,\n\
       init: fn(c: St) -> St ! {} = c,\n\
       handle Nap, st -> St ! {} = st,\n\
     }\n\
     supervisor NestSup {\n\
       strategy: one_for_one,\n\
       intensity: 2,\n\
       period: 30,\n\
       children: [\n\
         { id: planner, actor: Planner, start_args: let base = 1 in St(base), restart: permanent },\n\
         { id: worker, actor: Worker, start_args: let base = 2 in St(base + 1), restart: permanent },\n\
       ]\n\
     }";

const DRIFT: &str = "type St = St(Int)\n\
     actor Planner {\n\
       state: St,\n\
       message: PlannerMsg = | Nop,\n\
       init: fn(c: St) -> St ! {} = c,\n\
       handle Nop, st -> St ! {} = st,\n\
     }\n\
     supervisor DriftSup {\n\
       strategy: one_for_all,\n\
       intensity: 1,\n\
       period: 5,\n\
       children: [\n\
         { id: planner, actor: Planner, start_args: St(0), restart: permanent },\n\
       ]\n\
     }";

const IDLE: &str = "supervisor IdleSup {\n\
       strategy: one_for_one,\n\
       intensity: 1,\n\
       period: 5,\n\
       children: []\n\
     }";

const WIRE: &str = "effect Tool<t>\n\
     effect Exn<t>\n\
     type Option<a> = Some(a) | None\n\
     type HttpError = HttpError(Int, String)\n\
     type TicketId = TicketId(String)\n\
     tool CreateTicket : { title: String, body: String } -> TicketId\n\
     tool HttpGet : { url: String } -> Option<Int> ! {Exn<HttpError>}\n\
     tool Snapshot : { tags: List<String>, pair: (Int, Bool) } -> Float\n\
     fn file(t: String) -> TicketId ! {Tool<CreateTicket>} =\n\
       create_ticket({ title: t, body: \"filed\" })";

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

/// The emitted module named `module_name` (an actor's `gen_server` or a
/// supervisor's `supervisor` behaviour module).
fn behaviour_module(source: &str, name: &str, module_name: &str) -> String {
    emit_all(source, name)
        .into_iter()
        .find(|m| m.name == module_name)
        .expect("behaviour module is emitted")
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
fn snapshot_install_block_wraps_with_handlers() {
    insta::assert_snapshot!(emit(program("Deploy"), "Deploy"));
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
    insta::assert_snapshot!(behaviour_module(program("Boot"), "Boot", "hird_counter"));
}

#[test]
fn snapshot_actor_gen_server_call_and_reply() {
    insta::assert_snapshot!(behaviour_module(program("Msg"), "Msg", "hird_counter"));
}

#[test]
fn snapshot_actor_payload_and_tool_dispatch() {
    insta::assert_snapshot!(behaviour_module(
        program("Workshop"),
        "Workshop",
        "hird_worker"
    ));
}

#[test]
fn snapshot_actor_qualifies_module_functions() {
    insta::assert_snapshot!(behaviour_module(program("Clock"), "Clock", "hird_ticker"));
}

#[test]
fn snapshot_actor_multi_param_init() {
    insta::assert_snapshot!(emit_joined(program("Duo"), "Duo"));
}

#[test]
fn snapshot_actor_call_only_cast_fallback() {
    insta::assert_snapshot!(behaviour_module(program("Echo"), "Echo", "hird_responder"));
}

#[test]
fn snapshot_supervisor_single_child() {
    insta::assert_snapshot!(behaviour_module(
        program("Solo"),
        "Solo",
        "hird_planner_sup"
    ));
}

#[test]
fn snapshot_supervise_and_child_lookup() {
    insta::assert_snapshot!(emit(program("Tree"), "Tree"));
}

#[test]
fn snapshot_supervisor_multi_child_restart_dispositions() {
    insta::assert_snapshot!(behaviour_module(program("Fleet"), "Fleet", "hird_root_sup"));
}

#[test]
fn snapshot_supervisor_start_args_share_one_variable_scope() {
    insta::assert_snapshot!(behaviour_module(program("Nest"), "Nest", "hird_nest_sup"));
}

#[test]
fn snapshot_supervisor_strategy_emitted_verbatim() {
    insta::assert_snapshot!(behaviour_module(
        program("Drift"),
        "Drift",
        "hird_drift_sup"
    ));
}

#[test]
fn snapshot_supervisor_empty_children() {
    insta::assert_snapshot!(behaviour_module(program("Idle"), "Idle", "hird_idle_sup"));
}

#[test]
fn snapshot_tool_signature_table() {
    insta::assert_snapshot!(emit(program("Wire"), "Wire"));
}

// ── erlc validation ──────────────────────────────────────────────

/// Compiles every fixture's generated Erlang with stock `erlc`. Skipped (with
/// a note) when `erlc` is not on the `PATH`, unless `HIRD_REQUIRE_BEAM` is set
/// — where Erlang is meant to be installed, a missing toolchain is a failure,
/// not a quiet pass.
#[test]
fn generated_erlang_compiles_with_erlc() {
    let erlc = std::process::Command::new("erlc").arg("-version").output();
    if erlc.is_err() {
        assert!(
            std::env::var_os("HIRD_REQUIRE_BEAM").is_none(),
            "HIRD_REQUIRE_BEAM is set but erlc is not on PATH"
        );
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

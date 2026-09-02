// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The round-trip property: lowering is stable through pretty-printing.
//!
//! For a well-typed module, `source → check → lower → pretty_print → check →
//! lower` reproduces the first IR up to type-variable renaming. The property
//! catches pretty-printer bugs (output that fails to parse or re-check),
//! lowering bugs (information lost on the way down), and inference instability
//! (re-checking the printed form producing different types).
//!
//! Equality is taken modulo type-variable renaming: inference assigns fresh
//! variable identities on each run, and the printer may turn an inferred
//! signature into a skolemised one, so genuine unification variables and
//! skolem constants are both renumbered by first appearance before comparing.
//! Type and tool declarations are compared verbatim — their types are fixed
//! by the declared parameter names, with no inference freedom.

use hird_ast::{AstNode, SourceFile};
use hird_ir::{
    IrActorDef, IrActorHandler, IrActorInit, IrApp, IrArm, IrBindPat, IrChild, IrChildSpec,
    IrClock, IrConstructor, IrConstructorPat, IrCrash, IrDecl, IrExpr, IrExternRef, IrField,
    IrFnDef, IrHandle, IrHandleArm, IrInstall, IrLambda, IrLet, IrList, IrLiteral, IrLiteralPat,
    IrMatch, IrModule, IrParam, IrPattern, IrRecord, IrRecordField, IrReply, IrRequest, IrSchedule,
    IrSelf, IrSend, IrSpan, IrSpawn, IrStand, IrSupervise, IrSupervisorDef, IrTuple, IrTuplePat,
    IrVar, IrWildcardPat, lower_module, pretty_print,
};
use hird_types::{Effect, EffectRow, RowVar, Type};
use proptest::prelude::*;
use std::collections::BTreeMap;

// ── harness ──────────────────────────────────────────────────────

/// Parses, checks, and lowers `source`, panicking on any parse or type error
/// (a malformed program must surface immediately, not produce partial IR).
fn lower_src(source: &str, name: &str) -> IrModule {
    let parsed = hird_parse::parse(source, 0);
    assert!(
        parsed.is_ok(),
        "source failed to parse: {source}\n{:?}",
        parsed.diagnostics()
    );
    let file = SourceFile::cast(parsed.syntax().clone()).expect("root is a source file");
    let checked = hird_check::check(&file, 0);
    assert!(
        !checked.has_errors(),
        "source failed to type-check: {source}\n{:?}",
        checked.diagnostics
    );
    lower_module(&file, &checked, name)
}

/// Asserts the round-trip property for `source`: lowering it, pretty-printing,
/// and re-lowering the printed form yields a structurally equal IR (modulo
/// type-variable renaming).
fn assert_roundtrips(source: &str) {
    let first = lower_src(source, "M");
    let printed = pretty_print(&first);
    let second = lower_src(&printed, "M");
    assert_eq!(
        normalize(&first),
        normalize(&second),
        "round-trip changed the IR\n--- source ---\n{source}\n--- printed ---\n{printed}\n\
         --- first ---\n{first:#?}\n--- second ---\n{second:#?}"
    );
}

// ── type-variable normalisation ──────────────────────────────────
//
// A copy of the printer's canonicalisation, applied across a whole function or
// extern so two alpha-equivalent IRs become byte-for-byte equal.

/// Identity of a type variable: a unification index or a skolem name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum VarKey {
    /// A unification variable, by index.
    Unif(u32),
    /// A skolem constant, by name.
    Skolem(String),
}

/// Maps each type variable and row variable to its canonical index within one
/// declaration; the two kinds renumber in independent sequences.
#[derive(Default)]
struct VarMap {
    /// Type-variable identities to canonical indices.
    vars: BTreeMap<VarKey, u32>,
    /// Row-variable identities to canonical indices.
    rows: BTreeMap<RowVar, u32>,
}

impl VarMap {
    /// An empty map.
    fn new() -> Self {
        Self::default()
    }
}

/// The canonical index for type-variable `key`, allocating the next on first
/// sight.
fn intern(map: &mut VarMap, key: VarKey) -> u32 {
    let next = u32::try_from(map.vars.len()).unwrap_or(u32::MAX);
    *map.vars.entry(key).or_insert(next)
}

/// The canonical index for row variable `var`, allocating the next on first
/// sight.
fn intern_row(map: &mut VarMap, var: RowVar) -> u32 {
    let next = u32::try_from(map.rows.len()).unwrap_or(u32::MAX);
    *map.rows.entry(var).or_insert(next)
}

/// Whether a type name is a variable (lowercase) rather than a constructor.
fn is_type_var(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_lowercase)
}

/// `ty` with every variable (unification variable or skolem) renumbered by
/// first appearance through `map`.
fn canon_type(ty: &Type, map: &mut VarMap) -> Type {
    match ty {
        Type::TyVar(id) => Type::TyVar(intern(map, VarKey::Unif(*id))),
        Type::TyCon(name, args) if args.is_empty() && is_type_var(name.as_str()) => {
            Type::TyVar(intern(map, VarKey::Skolem(String::from(name.as_str()))))
        }
        Type::TyCon(name, args) => Type::TyCon(
            name.clone(),
            args.iter().map(|a| canon_type(a, map)).collect(),
        ),
        Type::TyFn(params, ret, row) => Type::TyFn(
            params.iter().map(|p| canon_type(p, map)).collect(),
            Box::new(canon_type(ret, map)),
            canon_effect_row(row, map),
        ),
        Type::TyTuple(elems) => Type::TyTuple(elems.iter().map(|e| canon_type(e, map)).collect()),
        Type::TyRecord(fields) => Type::TyRecord(
            fields
                .iter()
                .map(|(label, v)| (label.clone(), canon_type(v, map)))
                .collect(),
        ),
        Type::TyForall(tvars, rvars, body) => Type::TyForall(
            tvars
                .iter()
                .map(|v| intern(map, VarKey::Unif(*v)))
                .collect(),
            rvars
                .iter()
                .map(|v| RowVar::new(intern_row(map, *v)))
                .collect(),
            Box::new(canon_type(body, map)),
        ),
    }
}

/// Normalises an effect row's argument types and tail variable, mirroring the
/// printer so two alpha-equivalent rows compare equal.
fn canon_effect_row(row: &EffectRow, map: &mut VarMap) -> EffectRow {
    let mut out = EffectRow::empty();
    for effect in row.effects() {
        out.insert(canon_effect(effect, map));
    }
    out.with_tail(row.tail().map(|rv| RowVar::new(intern_row(map, rv))))
}

/// Normalises one effect's argument types.
fn canon_effect(effect: &Effect, map: &mut VarMap) -> Effect {
    match effect {
        Effect::Named(name) => Effect::Named(name.clone()),
        Effect::Parametric(name, args) => Effect::Parametric(
            name.clone(),
            args.iter().map(|a| canon_type(a, map)).collect(),
        ),
    }
}

/// A copy of `module` with each function's and extern's type variables
/// renumbered. Type declarations are left untouched.
fn normalize(module: &IrModule) -> IrModule {
    IrModule {
        name: module.name.clone(),
        declarations: module.declarations.iter().map(normalize_decl).collect(),
    }
}

/// Normalises one declaration's type variables (with a per-declaration map),
/// or returns a type declaration verbatim. Spans are cleared everywhere: the
/// printed source has its own layout, so positions never round-trip.
fn normalize_decl(decl: &IrDecl) -> IrDecl {
    match decl {
        IrDecl::Fn(f) => {
            let mut map = VarMap::new();
            IrDecl::Fn(IrFnDef {
                name: f.name.clone(),
                params: f.params.iter().map(|p| canon_param(p, &mut map)).collect(),
                return_type: canon_type(&f.return_type, &mut map),
                effect_row: canon_effect_row(&f.effect_row, &mut map),
                body: canon_expr(&f.body, &mut map),
                span: IrSpan::default(),
            })
        }
        // Type and tool declarations render under their declared parameter
        // names, with no inference freedom: compared verbatim (minus spans).
        IrDecl::Type(t) => IrDecl::Type(hird_ir::IrTypeDef {
            span: IrSpan::default(),
            ..t.clone()
        }),
        IrDecl::Tool(t) => IrDecl::Tool(hird_ir::IrToolDef {
            span: IrSpan::default(),
            ..t.clone()
        }),
        // An actor's interface types are concrete, but its bodies may bind
        // let-polymorphic values whose variable identities differ per run.
        IrDecl::Actor(a) => {
            let mut map = VarMap::new();
            IrDecl::Actor(IrActorDef {
                name: a.name.clone(),
                state: canon_type(&a.state, &mut map),
                message: hird_ir::IrTypeDef {
                    span: IrSpan::default(),
                    ..a.message.clone()
                },
                init: IrActorInit {
                    params: a
                        .init
                        .params
                        .iter()
                        .map(|p| canon_param(p, &mut map))
                        .collect(),
                    effect_row: canon_effect_row(&a.init.effect_row, &mut map),
                    body: canon_expr(&a.init.body, &mut map),
                },
                handlers: a
                    .handlers
                    .iter()
                    .map(|h| IrActorHandler {
                        message: canon_pattern(&h.message, &mut map),
                        state: canon_pattern(&h.state, &mut map),
                        effect_row: canon_effect_row(&h.effect_row, &mut map),
                        body: canon_expr(&h.body, &mut map),
                    })
                    .collect(),
                effect_row: canon_effect_row(&a.effect_row, &mut map),
                span: IrSpan::default(),
            })
        }
        IrDecl::Extern(e) => {
            let mut map = VarMap::new();
            IrDecl::Extern(IrExternRef {
                name: e.name.clone(),
                ty: canon_type(&e.ty, &mut map),
                module: e.module.clone(),
                span: IrSpan::default(),
            })
        }
        // A supervisor's only inference freedom is its children's `start_args`
        // and the derived effect row; the rest is fixed text.
        IrDecl::Supervisor(s) => {
            let mut map = VarMap::new();
            IrDecl::Supervisor(IrSupervisorDef {
                name: s.name.clone(),
                strategy: s.strategy.clone(),
                intensity: s.intensity,
                period: s.period,
                children: s
                    .children
                    .iter()
                    .map(|c| IrChildSpec {
                        id: c.id.clone(),
                        actor: c.actor.clone(),
                        start_args: canon_expr(&c.start_args, &mut map),
                        restart: c.restart.clone(),
                    })
                    .collect(),
                effect_row: canon_effect_row(&s.effect_row, &mut map),
                span: IrSpan::default(),
            })
        }
    }
}

/// Normalises a parameter's type.
fn canon_param(param: &IrParam, map: &mut VarMap) -> IrParam {
    IrParam {
        name: param.name.clone(),
        ty: canon_type(&param.ty, map),
    }
}

/// Normalises every type in an expression tree.
fn canon_expr(expr: &IrExpr, map: &mut VarMap) -> IrExpr {
    match expr {
        IrExpr::Literal(lit) => IrExpr::Literal(IrLiteral {
            value: lit.value.clone(),
            ty: canon_type(&lit.ty, map),
        }),
        IrExpr::Var(var) => IrExpr::Var(IrVar {
            name: var.name.clone(),
            ty: canon_type(&var.ty, map),
        }),
        IrExpr::Let(le) => IrExpr::Let(IrLet {
            name: le.name.clone(),
            ty: canon_type(&le.ty, map),
            value: Box::new(canon_expr(&le.value, map)),
            body: Box::new(canon_expr(&le.body, map)),
        }),
        IrExpr::Lambda(lambda) => IrExpr::Lambda(IrLambda {
            params: lambda.params.iter().map(|p| canon_param(p, map)).collect(),
            body: Box::new(canon_expr(&lambda.body, map)),
            body_type: canon_type(&lambda.body_type, map),
            effect_row: canon_effect_row(&lambda.effect_row, map),
        }),
        IrExpr::App(app) => IrExpr::App(IrApp {
            func: Box::new(canon_expr(&app.func, map)),
            args: app.args.iter().map(|a| canon_expr(a, map)).collect(),
            result_type: canon_type(&app.result_type, map),
        }),
        IrExpr::Match(m) => IrExpr::Match(IrMatch {
            scrutinee: Box::new(canon_expr(&m.scrutinee, map)),
            scrutinee_type: canon_type(&m.scrutinee_type, map),
            arms: m
                .arms
                .iter()
                .map(|arm| IrArm {
                    pattern: canon_pattern(&arm.pattern, map),
                    body: canon_expr(&arm.body, map),
                })
                .collect(),
            result_type: canon_type(&m.result_type, map),
        }),
        IrExpr::Handle(h) => IrExpr::Handle(IrHandle {
            arms: h
                .arms
                .iter()
                .map(|arm| IrHandleArm {
                    effect: canon_effect(&arm.effect, map),
                    handler: canon_expr(&arm.handler, map),
                })
                .collect(),
            body: Box::new(canon_expr(&h.body, map)),
            effect_row: canon_effect_row(&h.effect_row, map),
            result_type: canon_type(&h.result_type, map),
        }),
        IrExpr::Install(inst) => IrExpr::Install(IrInstall {
            arms: inst
                .arms
                .iter()
                .map(|arm| IrHandleArm {
                    effect: canon_effect(&arm.effect, map),
                    handler: canon_expr(&arm.handler, map),
                })
                .collect(),
            body: Box::new(canon_expr(&inst.body, map)),
            effect_row: canon_effect_row(&inst.effect_row, map),
            result_type: canon_type(&inst.result_type, map),
        }),
        IrExpr::Constructor(ctor) => IrExpr::Constructor(IrConstructor {
            name: ctor.name.clone(),
            type_name: ctor.type_name.clone(),
            args: ctor.args.iter().map(|a| canon_expr(a, map)).collect(),
            result_type: canon_type(&ctor.result_type, map),
        }),
        IrExpr::Tuple(tuple) => IrExpr::Tuple(IrTuple {
            elems: tuple.elems.iter().map(|e| canon_expr(e, map)).collect(),
            ty: canon_type(&tuple.ty, map),
        }),
        IrExpr::List(list) => IrExpr::List(IrList {
            elems: list.elems.iter().map(|e| canon_expr(e, map)).collect(),
            ty: canon_type(&list.ty, map),
        }),
        IrExpr::Record(record) => IrExpr::Record(IrRecord {
            fields: record
                .fields
                .iter()
                .map(|f| IrRecordField {
                    label: f.label.clone(),
                    value: canon_expr(&f.value, map),
                })
                .collect(),
            ty: canon_type(&record.ty, map),
        }),
        IrExpr::Field(field) => IrExpr::Field(IrField {
            receiver: Box::new(canon_expr(&field.receiver, map)),
            field: field.field.clone(),
            ty: canon_type(&field.ty, map),
        }),
        IrExpr::Spawn(spawn) => IrExpr::Spawn(IrSpawn {
            actor: spawn.actor.clone(),
            args: spawn.args.iter().map(|a| canon_expr(a, map)).collect(),
            result_type: canon_type(&spawn.result_type, map),
        }),
        IrExpr::Supervise(supervise) => IrExpr::Supervise(IrSupervise {
            supervisor: supervise.supervisor.clone(),
            result_type: canon_type(&supervise.result_type, map),
        }),
        IrExpr::Stand(stand) => IrExpr::Stand(IrStand {
            result_type: canon_type(&stand.result_type, map),
        }),
        IrExpr::Clock(clock) => IrExpr::Clock(IrClock {
            result_type: canon_type(&clock.result_type, map),
        }),
        IrExpr::SelfRef(this) => IrExpr::SelfRef(IrSelf {
            result_type: canon_type(&this.result_type, map),
        }),
        IrExpr::Schedule(schedule) => IrExpr::Schedule(IrSchedule {
            clock: Box::new(canon_expr(&schedule.clock, map)),
            pid: Box::new(canon_expr(&schedule.pid, map)),
            message: Box::new(canon_expr(&schedule.message, map)),
            delay: Box::new(canon_expr(&schedule.delay, map)),
            result_type: canon_type(&schedule.result_type, map),
        }),
        IrExpr::Child(child) => IrExpr::Child(IrChild {
            supervisor: child.supervisor.clone(),
            child_id: child.child_id.clone(),
            result_type: canon_type(&child.result_type, map),
        }),
        IrExpr::Send(send) => IrExpr::Send(IrSend {
            pid: Box::new(canon_expr(&send.pid, map)),
            message: Box::new(canon_expr(&send.message, map)),
            result_type: canon_type(&send.result_type, map),
        }),
        IrExpr::Request(request) => IrExpr::Request(IrRequest {
            pid: Box::new(canon_expr(&request.pid, map)),
            message_fn: Box::new(canon_expr(&request.message_fn, map)),
            timeout: request
                .timeout
                .as_ref()
                .map(|timeout| Box::new(canon_expr(timeout, map))),
            result_type: canon_type(&request.result_type, map),
        }),
        IrExpr::Reply(reply) => IrExpr::Reply(IrReply {
            reply_to: Box::new(canon_expr(&reply.reply_to, map)),
            value: Box::new(canon_expr(&reply.value, map)),
            result_type: canon_type(&reply.result_type, map),
        }),
        IrExpr::Crash(crash) => IrExpr::Crash(IrCrash {
            message: Box::new(canon_expr(&crash.message, map)),
            result_type: canon_type(&crash.result_type, map),
        }),
    }
}

/// Normalises every type in a pattern.
fn canon_pattern(pattern: &IrPattern, map: &mut VarMap) -> IrPattern {
    match pattern {
        IrPattern::Wildcard(w) => IrPattern::Wildcard(IrWildcardPat {
            ty: canon_type(&w.ty, map),
        }),
        IrPattern::Bind(b) => IrPattern::Bind(IrBindPat {
            name: b.name.clone(),
            ty: canon_type(&b.ty, map),
        }),
        IrPattern::Literal(l) => IrPattern::Literal(IrLiteralPat {
            value: l.value.clone(),
            ty: canon_type(&l.ty, map),
        }),
        IrPattern::Tuple(t) => IrPattern::Tuple(IrTuplePat {
            elems: t.elems.iter().map(|e| canon_pattern(e, map)).collect(),
            ty: canon_type(&t.ty, map),
        }),
        IrPattern::Constructor(c) => IrPattern::Constructor(IrConstructorPat {
            name: c.name.clone(),
            type_name: c.type_name.clone(),
            fields: c.fields.iter().map(|f| canon_pattern(f, map)).collect(),
            ty: canon_type(&c.ty, map),
        }),
    }
}

// ── hand-written round-trip programs ─────────────────────────────
//
// Each exercises distinct IR node kinds; together they cover every kind plus
// the desugarings (operators, `if`) and the printer's parenthesisation.

#[test]
fn operators_and_params() {
    assert_roundtrips("fn add(x: Int, y: Int) -> Int = x + y");
}

#[test]
fn polymorphic_signature_skolems() {
    // A signature variable used in two positions, written out of canonical
    // order to exercise the printer's per-signature renumbering.
    assert_roundtrips("fn snd(x: b, y: a) -> a = y");
}

#[test]
fn if_desugars_and_round_trips() {
    assert_roundtrips("fn pick(b: Bool) -> Int = if b then 1 else 2");
}

#[test]
fn adt_match_and_constructors() {
    assert_roundtrips(
        "type Option<a> = Some(a) | None\n\
         fn unwrap(opt: Option<Int>) -> Int = match opt { Some(x) -> x, None -> 0, }",
    );
}

#[test]
fn recursive_adt_and_nested_constructors() {
    assert_roundtrips(
        "type List<a> = Cons(a, List<a>) | Nil\n\
         fn build() -> List<Int> = Cons(1, Cons(2, Nil))",
    );
}

#[test]
fn let_polymorphic_lambda_and_application() {
    assert_roundtrips(r"fn use_id() -> Int = let id = \x -> x in id(1)");
}

#[test]
fn multi_parameter_lambda() {
    assert_roundtrips(r"fn const_fst() -> Int = (\x y -> x)(1, 2)");
}

#[test]
fn tuples_lists_unit_and_literals() {
    assert_roundtrips(
        "fn triple() -> (Int, String, Bool) = (1, \"a\", True)\n\
         fn nums() -> List<Int> = [1, 2, 3]\n\
         fn nothing() = ()\n\
         fn pi() -> Float = 3.14",
    );
}

#[test]
fn zero_argument_function_as_value() {
    // `get`'s return type is the zero-ary `() → Int`, which has no annotation
    // syntax (`() → Int` would re-parse as the one-argument `(()) → Int`), so
    // the printer must omit the return annotation and let inference recover it.
    assert_roundtrips("fn answer() -> Int = 42\nfn get() = answer");
}

#[test]
fn record_literal_and_field_access() {
    // `make` returns a record, whose type has no annotation syntax, so the
    // return annotation is omitted; `age` reads a field off a let-bound record.
    assert_roundtrips(
        "fn make() = { name: \"x\", age: 1 }\n\
         fn age() -> Int = let r = { name: \"x\", age: 1 } in r.age",
    );
}

#[test]
fn extern_reference() {
    assert_roundtrips("extern fn sqrt(x: Float) -> Float");
}

#[test]
fn polymorphic_extern() {
    assert_roundtrips("extern fn identity(x: a) -> a");
}

#[test]
fn literal_and_wildcard_patterns() {
    assert_roundtrips("fn classify(n: Int) -> Int = match n { 0 -> 100, 1 -> 200, _ -> 0, }");
}

#[test]
fn tuple_pattern() {
    assert_roundtrips("fn first(p: (Int, String)) -> Int = match p { (a, b) -> a, }");
}

#[test]
fn nested_operator_precedence() {
    // Mixed precedence and a non-associative comparison; the printer must
    // re-parenthesise to recover the same tree.
    assert_roundtrips("fn prec(a: Int, b: Int) -> Bool = (a + b) * 2 - 1 == b / 2");
}

#[test]
fn qualified_let_bindings() {
    // Sequential `let`s nest to the right; the printer must keep them parseable.
    assert_roundtrips(
        "fn chain() -> Int = let a = 1 in let b = a + 1 in let c = b + 1 in a + b + c",
    );
}

// ── effect rows ──────────────────────────────────────────────────
//
// Non-empty rows must survive the round-trip. The printer synthesises the
// `effect` declarations the rows reference, so the printed source re-checks
// even though effect declarations are not IR nodes.

#[test]
fn row_polymorphic_function() {
    // An open row variable, needing no effect declaration to re-check.
    assert_roundtrips("fn apply(g: a -> b ! {r}, x: a) -> b ! {r} = g(x)");
}

#[test]
fn single_named_effect() {
    // The body performs its declared effect by applying an effectful parameter.
    assert_roundtrips(
        "effect Log\n\
         fn log_it(run: Int -> Int ! {Log}) -> Int ! {Log} = run(0)",
    );
}

#[test]
fn multiple_and_parametric_effects() {
    assert_roundtrips(
        "effect Log\n\
         type Repo = MkRepo\n\
         fn read(run: Int -> Int ! {Log, Tool<Repo>}) -> Int ! {Log, Tool<Repo>} = run(0)",
    );
}

// ── handle blocks ────────────────────────────────────────────────
//
// A handle lowers to an `IrHandle` carrying its arms, body, and row. The
// printer re-emits the surface form, so the arms' effect heads and the body
// must survive the round-trip.

#[test]
fn handle_block_round_trips() {
    // Handling the body's `Tool<Repo>` with a logging handler trades it for
    // `Log`; the arm, the resulting row, and the tool declaration must all
    // come back unchanged.
    assert_roundtrips(
        "effect Log\n\
         tool Repo : { x: Int } -> Int\n\
         fn audited(f: Int -> Int ! {Tool<Repo>}, logh: { x: Int } -> Int ! {Log}) -> Int ! {Log} =\n\
           handle { Tool<Repo> -> logh } in f(0)",
    );
}

#[test]
fn handle_multi_arm_round_trips() {
    assert_roundtrips(
        "effect Log\n\
         tool Repo : { x: Int } -> Int\n\
         fn run(f: Int -> Int ! {Log, Tool<Repo>}, lh: Int -> Int, th: { x: Int } -> Int) -> Int =\n\
           handle { Log -> lh, Tool<Repo> -> th } in f(0)",
    );
}

#[test]
fn generic_tool_round_trips() {
    // The tool declaration itself must survive: its declared parameter, args
    // record, result, and trailing row are re-emitted and re-lowered intact.
    assert_roundtrips(
        "type Prompt = Prompt(String)\n\
         type Schema<t> = Schema(String)\n\
         type ParseError = ParseError(String)\n\
         tool LLMCall<t> : { prompt: Prompt, schema: Schema<t> } -> t ! {Exn<ParseError>}\n\
         fn ask(p: Prompt, s: Schema<Int>) -> Int ! {Exn<ParseError>, Tool<LLMCall>} =\n\
           llm_call({ prompt: p, schema: s })",
    );
}

#[test]
fn handle_effect_only_in_arm_round_trips() {
    // `Log` is named only by the handle arm — handling it leaves the function
    // pure — so the printer must synthesise `effect Log` from the body, not just
    // from signatures.
    assert_roundtrips(
        "effect Log\n\
         fn run(lh: Int -> Int) -> Int = handle { Log -> lh } in 0",
    );
}

// ── install blocks ───────────────────────────────────────────────
//
// An install lowers to an `IrInstall` carrying its arms, body, and row
// (body ∪ {Install}); the printer re-emits the surface form.

#[test]
fn install_block_round_trips() {
    assert_roundtrips(
        "tool Repo : { x: Int } -> Int\n\
         fn demo(f: Int -> Int ! {Tool<Repo>}, h: { x: Int } -> Int) -> Int ! {Install, Tool<Repo>} =\n\
           install { Tool<Repo> -> h } in f(0)",
    );
}

#[test]
fn install_multi_arm_round_trips() {
    assert_roundtrips(
        "effect Log\n\
         tool Repo : { x: Int } -> Int\n\
         fn demo(f: Int -> Int ! {Log}, lh: Int -> Int, th: { x: Int } -> Int) -> Int ! {Install, Log} =\n\
           install { Log -> lh, Tool<Repo> -> th } in f(0)",
    );
}

// ── pretty-printer snapshots ─────────────────────────────────────

#[test]
fn snapshot_adt_and_match() {
    let module = lower_src(
        "type Option<a> = Some(a) | None\n\
         fn unwrap(opt: Option<Int>) -> Int = match opt { Some(x) -> x, None -> 0, }",
        "Opt",
    );
    insta::assert_snapshot!(pretty_print(&module));
}

#[test]
fn snapshot_let_lambda_and_operators() {
    let module = lower_src(
        r"fn compute(a: Int, b: Int) -> Int = let scaled = (a + b) * 2 in scaled - 1",
        "Calc",
    );
    insta::assert_snapshot!(pretty_print(&module));
}

#[test]
fn snapshot_polymorphic_and_extern() {
    let module = lower_src(
        "extern fn map(f: a -> b, xs: List<a>) -> List<b>\n\
         fn snd(x: b, y: a) -> a = y",
        "Poly",
    );
    insta::assert_snapshot!(pretty_print(&module));
}

#[test]
fn snapshot_handle_block() {
    // The handled `Tool<Repo>` leaves the row, the handler's `Log` joins it,
    // and the printer re-emits the `handle { … } in …` surface form plus the
    // tool declaration backing the arm.
    let module = lower_src(
        "effect Log\n\
         tool Repo : { x: Int } -> Int\n\
         fn audited(f: Int -> Int ! {Tool<Repo>}, logh: { x: Int } -> Int ! {Log}) -> Int ! {Log} =\n\
           handle { Tool<Repo> -> logh } in f(0)",
        "Handle",
    );
    insta::assert_snapshot!(pretty_print(&module));
}

#[test]
fn actor_round_trips() {
    assert_roundtrips(
        "type Path = Path(String)\n\
         type St = St(Int)\n\
         tool ReadRepo : { path: Path } -> St\n\
         actor Planner {\n\
           state: St,\n\
           message: PlannerMsg = | Plan(Path) | Get(ReplyTo<St>) | Quit,\n\
           init: fn(start: St) -> St ! {} = start,\n\
           handle Plan(p), st -> Next<St> ! {Tool<ReadRepo>} = Continue(read_repo({ path: p })),\n\
           handle Get(reply_to), St(n) -> Next<St> ! {} = Continue(St(n)),\n\
           handle Quit, st -> Next<St> ! {} = Continue(st),\n\
         } ! {Tool<ReadRepo>}",
    );
}

#[test]
fn spawn_round_trips() {
    assert_roundtrips(
        "type St = St(Int)\n\
         actor Counter {\n\
           state: St,\n\
           message: Msg = | Inc,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Inc, St(n) -> Next<St> ! {} = Continue(St(n + 1)),\n\
         }\n\
         fn boot(s: St) -> Pid<Msg> ! {Spawn<Msg>} = spawn(Counter, s)",
    );
}

#[test]
fn supervision_round_trips() {
    assert_roundtrips(
        "type St = St(Int)\n\
         fn config() -> St = St(0)\n\
         actor Counter {\n\
           state: St,\n\
           message: Msg = | Inc,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Inc, St(n) -> Next<St> ! {} = Continue(St(n + 1)),\n\
         }\n\
         supervisor CounterSup {\n\
           strategy: one_for_one,\n\
           intensity: 5,\n\
           period: 60,\n\
           children: [\n\
             { id: counter, actor: Counter, start_args: config(), restart: permanent },\n\
           ]\n\
         }\n\
         fn boot() ! {Supervise} = supervise(CounterSup)\n\
         fn counter() -> Pid<Msg> ! {} = child(CounterSup, counter)\n\
         fn serve() ! {Supervise, Stand} = let u = supervise(CounterSup) in stand()",
    );
}

#[test]
fn messaging_round_trips() {
    assert_roundtrips(
        "type Status = Status(Int)\n\
         type St = St(Int)\n\
         actor Counter {\n\
           state: St,\n\
           message: Msg = | Inc | Get(ReplyTo<Status>),\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Inc, St(n) -> Next<St> ! {} = Continue(St(n + 1)),\n\
           handle Get(r), St(n) -> Next<St> ! {Send<Status>} = let sent = reply(r, Status(n)) in Continue(St(n)),\n\
         } ! {Send<Status>}\n\
         fn poke(p: Pid<Msg>) ! {Send<Msg>} = send(p, Inc)\n\
         fn query(p: Pid<Msg>) -> Status ! {Send<Msg>, Await<Status>} = request(p, Get)\n\
         fn patient(p: Pid<Msg>) -> Status ! {Send<Msg>, Await<Status>} = request(p, Get, 60000)",
    );
}

#[test]
fn time_round_trips() {
    assert_roundtrips(
        "type Cfg = Cfg(Clock, Int)\n\
         actor Heart {\n\
           state: Cfg,\n\
           message: HeartMsg = | Beat,\n\
           init: fn(c: Cfg) -> Cfg ! {Schedule<HeartMsg>} =\n\
             match c { Cfg(clock, period) -> let first = schedule(clock, self(), Beat, period) in c },\n\
           handle Beat, Cfg(clock, period) -> Next<Cfg> ! {Schedule<HeartMsg>} =\n\
             let next = schedule(clock, self(), Beat, period) in Continue(Cfg(clock, period)),\n\
         } ! {Schedule<HeartMsg>}\n\
         supervisor HeartSup {\n\
           strategy: one_for_one,\n\
           intensity: 5,\n\
           period: 60,\n\
           children: [\n\
             { id: heart, actor: Heart, start_args: Cfg(clock(), 1000), restart: permanent },\n\
           ]\n\
         }\n\
         fn kick(p: Pid<HeartMsg>) ! {Clock, Schedule<HeartMsg>} = schedule(clock(), p, Beat, 10)",
    );
}

#[test]
fn supervisor_round_trips() {
    // The supervisor prints without an effect annotation (its row is derived),
    // so re-checking re-derives the same row; the child's identifiers and pure
    // `start_args` must come back unchanged.
    assert_roundtrips(
        "type Path = Path(String)\n\
         type St = St(Int)\n\
         tool ReadRepo : { path: Path } -> St\n\
         fn planner_config() -> St = St(0)\n\
         actor Planner {\n\
           state: St,\n\
           message: Msg = | Plan(Path) | Quit,\n\
           init: fn(c: St) -> St ! {} = c,\n\
           handle Plan(p), st -> Next<St> ! {Tool<ReadRepo>} = Continue(read_repo({ path: p })),\n\
           handle Quit, st -> Next<St> ! {} = Continue(st),\n\
         } ! {Tool<ReadRepo>}\n\
         supervisor PlannerSup {\n\
           strategy: one_for_one,\n\
           intensity: 5,\n\
           period: 60,\n\
           children: [\n\
             { id: planner, actor: Planner, start_args: planner_config(), restart: permanent },\n\
           ]\n\
         }",
    );
}

#[test]
fn multi_child_supervisor_round_trips() {
    // Two children with distinct actors and restart dispositions; the derived
    // row is the union of both per-actor summaries.
    assert_roundtrips(
        "type Path = Path(String)\n\
         type Title = Title(String)\n\
         type St = St(Int)\n\
         tool ReadRepo : { path: Path } -> St\n\
         tool CreateTicket : { title: Title } -> St\n\
         fn planner_config() -> St = St(0)\n\
         fn worker_config() -> St = St(1)\n\
         actor Planner {\n\
           state: St,\n\
           message: PMsg = | Plan(Path),\n\
           init: fn(c: St) -> St ! {} = c,\n\
           handle Plan(p), st -> Next<St> ! {Tool<ReadRepo>} = Continue(read_repo({ path: p })),\n\
         } ! {Tool<ReadRepo>}\n\
         actor Worker {\n\
           state: St,\n\
           message: WMsg = | Work(Title),\n\
           init: fn(c: St) -> St ! {} = c,\n\
           handle Work(t), st -> Next<St> ! {Tool<CreateTicket>} = Continue(create_ticket({ title: t })),\n\
         } ! {Tool<CreateTicket>}\n\
         supervisor RootSup {\n\
           strategy: one_for_one,\n\
           intensity: 3,\n\
           period: 10,\n\
           children: [\n\
             { id: planner, actor: Planner, start_args: planner_config(), restart: permanent },\n\
             { id: worker, actor: Worker, start_args: worker_config(), restart: transient },\n\
           ]\n\
         }",
    );
}

#[test]
fn snapshot_supervisor_declaration() {
    // The printed supervisor keeps its strategy, restart budget, and typed
    // children, and omits the derived effect row.
    let module = lower_src(
        "type Path = Path(String)\n\
         type St = St(Int)\n\
         tool ReadRepo : { path: Path } -> St\n\
         fn planner_config() -> St = St(0)\n\
         actor Planner {\n\
           state: St,\n\
           message: Msg = | Plan(Path) | Quit,\n\
           init: fn(c: St) -> St ! {} = c,\n\
           handle Plan(p), st -> Next<St> ! {Tool<ReadRepo>} = Continue(read_repo({ path: p })),\n\
           handle Quit, st -> Next<St> ! {} = Continue(st),\n\
         } ! {Tool<ReadRepo>}\n\
         supervisor PlannerSup {\n\
           strategy: one_for_one,\n\
           intensity: 5,\n\
           period: 60,\n\
           children: [\n\
             { id: planner, actor: Planner, start_args: planner_config(), restart: permanent },\n\
           ]\n\
         }",
        "Sup",
    );
    insta::assert_snapshot!(pretty_print(&module));
}

#[test]
fn snapshot_actor_declaration() {
    // The printed actor re-declares its message sum type inline, keeps the
    // trailing effect summary, and elides empty member rows.
    let module = lower_src(
        "type Path = Path(String)\n\
         type St = St(Int)\n\
         tool ReadRepo : { path: Path } -> St\n\
         actor Planner {\n\
           state: St,\n\
           message: PlannerMsg = | Plan(Path) | Quit,\n\
           init: fn(start: St) -> St ! {} = start,\n\
           handle Plan(p), st -> Next<St> ! {Tool<ReadRepo>} = Continue(read_repo({ path: p })),\n\
           handle Quit, st -> Next<St> ! {} = Continue(st),\n\
         } ! {Tool<ReadRepo>}",
        "Actors",
    );
    insta::assert_snapshot!(pretty_print(&module));
}

#[test]
fn snapshot_effects_synthesise_declarations() {
    // Effect declarations are reconstructed from the rows that reference them,
    // and a row variable prints as an open row.
    let module = lower_src(
        "effect Log\n\
         type Repo = MkRepo\n\
         fn read(run: Int -> Int ! {Log, Tool<Repo>}) -> Int ! {Log, Tool<Repo>} = run(0)\n\
         fn apply(g: a -> b ! {r}, x: a) -> b ! {r} = g(x)",
        "Eff",
    );
    insta::assert_snapshot!(pretty_print(&module));
}

// ── crash primitive ──────────────────────────────────────────────
//
// `crash!` lowers to an `IrExpr::Crash` and re-emits as `crash!(…)`. `panic!`
// is a surface alias with no IR of its own, so it prints as `crash!` — the IR,
// not the source, is what the round-trip fixes.

#[test]
fn crash_round_trips() {
    assert_roundtrips(r#"fn boom() -> Int = crash!("nope")"#);
}

#[test]
fn panic_round_trips() {
    assert_roundtrips(r#"fn boom() -> String = panic!("nope")"#);
}

#[test]
fn crash_in_match_arm_round_trips() {
    assert_roundtrips(
        "type Option<a> = Some(a) | None\n\
         fn unwrap(o: Option<Int>) -> Int = match o { Some(x) -> x, None -> crash!(\"empty\"), }",
    );
}

// ── generated round-trip programs ────────────────────────────────
//
// Terms are built type-directed: each generator node knows the type it
// produces, so the rendered `fn main` is well-typed by construction. Binder
// names come from a counter, so no two binders collide.

/// The scalar types terms are generated at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    /// `Int`.
    Int,
    /// `String`.
    Str,
    /// `Bool`.
    Bool,
}

/// A well-typed term, tagged with the type each node produces.
#[derive(Debug, Clone)]
enum Term {
    /// An integer literal.
    IntLit(u8),
    /// A string literal `"s<n>"`.
    StrLit(u8),
    /// A `Bool` constructor.
    BoolLit(bool),
    /// `if c then a else b`, both branches at the target type.
    If(Box<Self>, Box<Self>, Box<Self>),
    /// Integer addition.
    Add(Box<Self>, Box<Self>),
    /// Integer comparison producing `Bool`.
    Lt(Box<Self>, Box<Self>),
    /// Polymorphic equality at the carried operand type.
    Eq(Box<Self>, Box<Self>),
    /// `let v<n> = value in body`; the binder is deliberately unused.
    Let(Box<Self>, Box<Self>),
    /// The identity lambda applied immediately: `(\p<n> -> p<n>)(e)`.
    IdApp(Box<Self>),
    /// A polymorphic let used at the target type:
    /// `let f<n> = \x<n> -> x<n> in f<n>(e)`.
    PolyLet(Box<Self>),
    /// Tuple destructuring: `match (1, e) { (a<n>, b<n>) -> b<n>, }`.
    MatchSnd(Box<Self>),
}

/// A leaf term of type `ty`.
fn leaf(ty: Ty) -> BoxedStrategy<Term> {
    match ty {
        Ty::Int => (0..100_u8).prop_map(Term::IntLit).boxed(),
        Ty::Str => (0..10_u8).prop_map(Term::StrLit).boxed(),
        Ty::Bool => any::<bool>().prop_map(Term::BoolLit).boxed(),
    }
}

/// A term of type `ty` with nesting bounded by `depth`.
fn term(ty: Ty, depth: u32) -> BoxedStrategy<Term> {
    if depth == 0 {
        return leaf(ty);
    }
    let d = depth - 1;
    let mut options: Vec<BoxedStrategy<Term>> = vec![
        leaf(ty),
        (term(Ty::Bool, d), term(ty, d), term(ty, d))
            .prop_map(|(c, a, b)| Term::If(Box::new(c), Box::new(a), Box::new(b)))
            .boxed(),
        (any_ty(), term(ty, d))
            .prop_flat_map(move |(value_ty, body)| {
                term(value_ty, d)
                    .prop_map(move |value| Term::Let(Box::new(value), Box::new(body.clone())))
            })
            .boxed(),
        term(ty, d).prop_map(|e| Term::IdApp(Box::new(e))).boxed(),
        term(ty, d).prop_map(|e| Term::PolyLet(Box::new(e))).boxed(),
        term(ty, d)
            .prop_map(|e| Term::MatchSnd(Box::new(e)))
            .boxed(),
    ];
    match ty {
        Ty::Int => options.push(
            (term(Ty::Int, d), term(Ty::Int, d))
                .prop_map(|(l, r)| Term::Add(Box::new(l), Box::new(r)))
                .boxed(),
        ),
        Ty::Bool => {
            options.push(
                (term(Ty::Int, d), term(Ty::Int, d))
                    .prop_map(|(l, r)| Term::Lt(Box::new(l), Box::new(r)))
                    .boxed(),
            );
            options.push(
                any_ty()
                    .prop_flat_map(move |operand_ty| {
                        (term(operand_ty, d), term(operand_ty, d))
                            .prop_map(|(l, r)| Term::Eq(Box::new(l), Box::new(r)))
                    })
                    .boxed(),
            );
        }
        Ty::Str => {}
    }
    proptest::strategy::Union::new(options).boxed()
}

/// One of the three scalar types.
fn any_ty() -> BoxedStrategy<Ty> {
    prop_oneof![Just(Ty::Int), Just(Ty::Str), Just(Ty::Bool)].boxed()
}

/// Renders `term` to surface syntax, drawing binder names from `next`. Every
/// composite is parenthesised, so the generated source's precedence is never
/// in question (the printer's own parenthesisation is exercised separately).
fn render(term: &Term, next: &mut u32, out: &mut String) {
    use std::fmt::Write as _;

    /// The next unique binder suffix.
    fn bump(next: &mut u32) -> u32 {
        let n = *next;
        *next += 1;
        n
    }

    match term {
        Term::IntLit(v) => write!(out, "{v}").unwrap(),
        Term::StrLit(v) => write!(out, "\"s{v}\"").unwrap(),
        Term::BoolLit(true) => out.push_str("True"),
        Term::BoolLit(false) => out.push_str("False"),
        Term::If(c, a, b) => {
            out.push_str("(if ");
            render(c, next, out);
            out.push_str(" then ");
            render(a, next, out);
            out.push_str(" else ");
            render(b, next, out);
            out.push(')');
        }
        Term::Add(l, r) | Term::Lt(l, r) | Term::Eq(l, r) => {
            let op = match term {
                Term::Add(..) => "+",
                Term::Lt(..) => "<",
                _ => "==",
            };
            out.push('(');
            render(l, next, out);
            write!(out, " {op} ").unwrap();
            render(r, next, out);
            out.push(')');
        }
        Term::Let(value, body) => {
            let n = bump(next);
            write!(out, "(let v{n} = ").unwrap();
            render(value, next, out);
            out.push_str(" in ");
            render(body, next, out);
            out.push(')');
        }
        Term::IdApp(e) => {
            let n = bump(next);
            write!(out, "((\\p{n} -> p{n})(").unwrap();
            render(e, next, out);
            out.push_str("))");
        }
        Term::PolyLet(e) => {
            let n = bump(next);
            write!(out, "(let f{n} = \\x{n} -> x{n} in f{n}(").unwrap();
            render(e, next, out);
            out.push_str("))");
        }
        Term::MatchSnd(e) => {
            let n = bump(next);
            out.push_str("(match (1, ");
            render(e, next, out);
            write!(out, ") {{ (a{n}, b{n}) -> b{n}, }})").unwrap();
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_programs_round_trip(t in any_ty().prop_flat_map(|ty| term(ty, 3))) {
        let mut source = String::from("fn main() = ");
        let mut next = 0;
        render(&t, &mut next, &mut source);
        assert_roundtrips(&source);
    }
}

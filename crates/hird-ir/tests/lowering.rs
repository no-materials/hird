// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Lowering coverage: distinct typed programs lowered to IR, with the
//! structure and the JSON projection checked directly.

use hird_ast::{AstNode, SourceFile};
use hird_ir::{IrDecl, IrExpr, IrModule, IrPattern, LiteralValue, lower_module};

/// Parses, checks, and lowers `source` into a module named `name`. Panics on
/// any parse or type error so a malformed test surfaces immediately.
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
    lower_module(&file, &checked, name)
}

/// The single function definition of a one-function module.
fn only_fn(module: &IrModule) -> &hird_ir::IrFnDef {
    let [IrDecl::Fn(f)] = module.declarations.as_slice() else {
        panic!(
            "expected exactly one function, got {:?}",
            module.declarations
        );
    };
    f
}

/// The function definition named `name`.
fn fn_named<'a>(module: &'a IrModule, name: &str) -> &'a hird_ir::IrFnDef {
    module
        .declarations
        .iter()
        .find_map(|d| match d {
            IrDecl::Fn(f) if f.name == name => Some(f),
            _ => None,
        })
        .expect("function is present")
}

/// Renders a type display-canonically.
fn ty_str(ty: &hird_types::Type) -> String {
    format!("{ty}")
}

// ── operators desugar to application ─────────────────────────────

#[test]
fn binary_operator_lowers_to_application() {
    let module = lower("fn add(x: Int, y: Int) -> Int = x + y", "Math");
    let add = only_fn(&module);

    assert_eq!(add.name, "add");
    assert_eq!(add.params.len(), 2);
    assert_eq!(add.params[0].name, "x");
    assert_eq!(ty_str(&add.params[0].ty), "Int");
    assert_eq!(ty_str(&add.return_type), "Int");
    // Empty effect row for now.
    assert_eq!(add.effect_row, hird_ir::EffectRow::empty());

    // `x + y` becomes application of the `+` primitive.
    let IrExpr::App(app) = &add.body else {
        panic!(
            "operator body should lower to application, got {:?}",
            add.body
        );
    };
    let IrExpr::Var(op) = app.func.as_ref() else {
        panic!("callee should be the operator reference");
    };
    assert_eq!(op.name, "+");
    assert_eq!(ty_str(&op.ty), "Int \u{2192} Int \u{2192} Int");
    assert_eq!(app.args.len(), 2);
    assert_eq!(ty_str(&app.result_type), "Int");
    let (IrExpr::Var(lhs), IrExpr::Var(rhs)) = (&app.args[0], &app.args[1]) else {
        panic!("operands should be variable references");
    };
    assert_eq!(lhs.name, "x");
    assert_eq!(rhs.name, "y");
}

#[test]
fn logical_operator_normalises_to_unicode() {
    // Written ASCII; the operator reference is the canonical Unicode form.
    let module = lower("fn both(p: Bool, q: Bool) -> Bool = p && q", "Logic");
    let IrExpr::App(app) = &only_fn(&module).body else {
        panic!("expected application");
    };
    let IrExpr::Var(op) = app.func.as_ref() else {
        panic!("expected operator reference");
    };
    assert_eq!(op.name, "\u{2227}", "`&&` canonicalises to `\u{2227}`");
}

// ── let, lambda, application ─────────────────────────────────────

#[test]
fn let_pattern_lowers_to_one_arm_match() {
    // A destructuring binder has no IR node of its own: it is the one-arm
    // match the checker proved total.
    let module = lower(
        "type Cfg = Cfg(Int, String)\n\
         fn main() = let Cfg(n, _) = Cfg(1, \"x\") in n",
        "Main",
    );
    let main = fn_named(&module, "main");
    let IrExpr::Match(m) = &main.body else {
        panic!("body should be a match, got {:?}", main.body);
    };
    assert_eq!(m.arms.len(), 1);
    assert!(matches!(&m.arms[0].pattern, IrPattern::Constructor(c) if c.name == "Cfg"));
    assert!(matches!(&m.arms[0].body, IrExpr::Var(v) if v.name == "n"));
}

#[test]
fn sequence_lowers_to_discard_let() {
    // `a; b` is the discard let it abbreviates.
    let module = lower(
        "effect Log\n\
         fn f(run: Int -> () ! {Log}) -> Int ! {Log} = run(0); 1",
        "Main",
    );
    let IrExpr::Let(le) = &only_fn(&module).body else {
        panic!("body should be a let");
    };
    assert_eq!(le.name, "_");
    assert!(matches!(le.value.as_ref(), IrExpr::App(_)));
}

#[test]
fn let_wildcard_lowers_to_discard_let() {
    // `let _ = e in b` is a let named `_`, not a one-arm match: codegen emits
    // `_ = E` for it.
    let module = lower(
        "effect Log\n\
         fn f(run: Int -> Int ! {Log}) -> Int ! {Log} = let _ = run(0) in 1",
        "Main",
    );
    let IrExpr::Let(le) = &only_fn(&module).body else {
        panic!("body should be a let");
    };
    assert_eq!(le.name, "_");
    assert!(matches!(le.value.as_ref(), IrExpr::App(_)));
}

#[test]
fn let_lambda_and_application() {
    let module = lower(r"fn main() = let id = \x -> x in id(1)", "Main");
    let main = only_fn(&module);
    assert!(main.params.is_empty());

    let IrExpr::Let(le) = &main.body else {
        panic!("body should be a let, got {:?}", main.body);
    };
    assert_eq!(le.name, "id");

    // The bound value is the identity lambda; its body reuses the parameter,
    // so the two share a type.
    let IrExpr::Lambda(lambda) = le.value.as_ref() else {
        panic!("let value should be a lambda");
    };
    assert_eq!(lambda.params.len(), 1);
    assert_eq!(lambda.params[0].name, "x");
    let IrExpr::Var(body_var) = lambda.body.as_ref() else {
        panic!("lambda body should be a variable");
    };
    assert_eq!(body_var.name, "x");
    assert_eq!(
        body_var.ty, lambda.params[0].ty,
        "the body variable has the parameter's type"
    );
    assert_eq!(body_var.ty, lambda.body_type);

    // The let body applies `id` to `1`, instantiated at `Int → Int`.
    let IrExpr::App(app) = le.body.as_ref() else {
        panic!("let body should be an application");
    };
    let IrExpr::Var(callee) = app.func.as_ref() else {
        panic!("callee should be a variable");
    };
    assert_eq!(callee.name, "id");
    assert_eq!(ty_str(&callee.ty), "Int \u{2192} Int");
    assert_eq!(ty_str(&app.result_type), "Int");
    let [IrExpr::Literal(one)] = app.args.as_slice() else {
        panic!("one integer argument");
    };
    assert_eq!(one.value, LiteralValue::Int("1".into()));
    assert_eq!(ty_str(&one.ty), "Int");
}

// ── ADTs, constructors, and match ────────────────────────────────

#[test]
fn adt_constructors_and_match() {
    let module = lower(
        "type Option<a> = Some(a) | None\n\
         fn unwrap(opt: Option<Int>) -> Int = match opt { Some(x) -> x, None -> 0, }",
        "Opt",
    );

    // The type definition: one parameter, two constructors, the field of
    // `Some` rendered with the declared parameter name.
    let IrDecl::Type(def) = &module.declarations[0] else {
        panic!("first declaration is the type");
    };
    assert_eq!(def.name, "Option");
    assert_eq!(def.params, ["a"]);
    assert_eq!(def.constructors.len(), 2);
    assert_eq!(def.constructors[0].name, "Some");
    assert_eq!(ty_str(&def.constructors[0].fields[0]), "a");
    assert_eq!(def.constructors[1].name, "None");
    assert!(def.constructors[1].fields.is_empty());

    let IrDecl::Fn(unwrap) = &module.declarations[1] else {
        panic!("second declaration is the function");
    };
    let IrExpr::Match(me) = &unwrap.body else {
        panic!("body should be a match");
    };
    assert_eq!(ty_str(&me.scrutinee_type), "Option<Int>");
    assert_eq!(ty_str(&me.result_type), "Int");
    assert_eq!(me.arms.len(), 2);

    // First arm: `Some(x) -> x`.
    let IrPattern::Constructor(some_pat) = &me.arms[0].pattern else {
        panic!("first arm matches a constructor");
    };
    assert_eq!(some_pat.name, "Some");
    assert_eq!(some_pat.type_name, "Option");
    assert_eq!(ty_str(&some_pat.ty), "Option<Int>");
    let [IrPattern::Bind(bind)] = some_pat.fields.as_slice() else {
        panic!("`Some` binds one field");
    };
    assert_eq!(bind.name, "x");
    assert_eq!(ty_str(&bind.ty), "Int");

    // Second arm: `None -> 0`; the nullary constructor knows its owner.
    let IrPattern::Constructor(none_pat) = &me.arms[1].pattern else {
        panic!("second arm matches a constructor");
    };
    assert_eq!(none_pat.name, "None");
    assert_eq!(none_pat.type_name, "Option");
    assert!(none_pat.fields.is_empty());
    let IrExpr::Literal(zero) = &me.arms[1].body else {
        panic!("`None` arm body is the literal 0");
    };
    assert_eq!(zero.value, LiteralValue::Int("0".into()));
}

#[test]
fn nullary_constructor_reference_knows_its_type() {
    let module = lower(
        "type Option<a> = Some(a) | None\n\
         fn nothing() -> Option<Int> = None",
        "Opt",
    );
    let IrDecl::Fn(f) = &module.declarations[1] else {
        panic!("second declaration is the function");
    };
    let IrExpr::Constructor(ctor) = &f.body else {
        panic!("body is the `None` constructor, got {:?}", f.body);
    };
    assert_eq!(ctor.name, "None");
    assert_eq!(ctor.type_name, "Option");
    assert!(ctor.args.is_empty());
    assert_eq!(ty_str(&ctor.result_type), "Option<Int>");
}

#[test]
fn recursive_adt_field_types_use_parameter_names() {
    let module = lower(
        "type List<a> = Cons(a, List<a>) | Nil\n\
         fn build() = Cons(1, Cons(2, Nil))",
        "Lst",
    );

    let IrDecl::Type(def) = &module.declarations[0] else {
        panic!("first declaration is the type");
    };
    assert_eq!(def.params, ["a"]);
    let cons = &def.constructors[0];
    assert_eq!(cons.name, "Cons");
    assert_eq!(ty_str(&cons.fields[0]), "a");
    assert_eq!(ty_str(&cons.fields[1]), "List<a>");

    // The body is a nested constructor application.
    let IrDecl::Fn(build) = &module.declarations[1] else {
        panic!("second declaration is the function");
    };
    let IrExpr::Constructor(outer) = &build.body else {
        panic!("body is a `Cons` application");
    };
    assert_eq!(outer.name, "Cons");
    assert_eq!(outer.type_name, "List");
    assert_eq!(outer.args.len(), 2);
    assert_eq!(ty_str(&outer.result_type), "List<Int>");
    let IrExpr::Constructor(inner) = &outer.args[1] else {
        panic!("second argument is the nested `Cons`");
    };
    assert_eq!(inner.name, "Cons");
}

// ── if desugars to match over Bool ───────────────────────────────

#[test]
fn if_desugars_to_match_over_bool() {
    let module = lower("fn pick(b: Bool) -> Int = if b then 1 else 2", "Cond");
    let IrExpr::Match(me) = &only_fn(&module).body else {
        panic!(
            "`if` should desugar to a match, got {:?}",
            only_fn(&module).body
        );
    };

    assert_eq!(ty_str(&me.scrutinee_type), "Bool");
    assert_eq!(ty_str(&me.result_type), "Int");

    // Two synthetic arms: `True -> 1`, `False -> 2`.
    let names: Vec<&str> = me
        .arms
        .iter()
        .map(|arm| match &arm.pattern {
            IrPattern::Constructor(c) => c.name.as_str(),
            other => panic!("if-arm should be a constructor pattern, got {other:?}"),
        })
        .collect();
    assert_eq!(names, ["True", "False"]);

    let IrExpr::Var(scrutinee) = me.scrutinee.as_ref() else {
        panic!("scrutinee is the condition variable");
    };
    assert_eq!(scrutinee.name, "b");
    let IrExpr::Literal(then_lit) = &me.arms[0].body else {
        panic!("then-branch is the literal 1");
    };
    assert_eq!(then_lit.value, LiteralValue::Int("1".into()));
}

// ── tuples, lists, unit, and literals ────────────────────────────

#[test]
fn tuple_list_unit_and_literals() {
    let module = lower(
        "fn triple() = (1, \"a\", True)\n\
         fn nums() = [1, 2, 3]\n\
         fn nothing() = ()",
        "Lits",
    );

    let IrDecl::Fn(triple) = &module.declarations[0] else {
        panic!("triple");
    };
    let IrExpr::Tuple(tuple) = &triple.body else {
        panic!("body is a tuple");
    };
    assert_eq!(tuple.elems.len(), 3);
    assert_eq!(ty_str(&tuple.ty), "(Int, String, Bool)");
    let IrExpr::Literal(s) = &tuple.elems[1] else {
        panic!("second element is a string literal");
    };
    assert_eq!(s.value, LiteralValue::Str("\"a\"".into()));
    let IrExpr::Constructor(t) = &tuple.elems[2] else {
        panic!("third element is the True constructor");
    };
    assert_eq!(t.name, "True");
    assert_eq!(t.type_name, "Bool");

    let IrDecl::Fn(nums) = &module.declarations[1] else {
        panic!("nums");
    };
    let IrExpr::List(list) = &nums.body else {
        panic!("body is a list");
    };
    assert_eq!(list.elems.len(), 3);
    assert_eq!(ty_str(&list.ty), "List<Int>");

    let IrDecl::Fn(nothing) = &module.declarations[2] else {
        panic!("nothing");
    };
    let IrExpr::Tuple(unit) = &nothing.body else {
        panic!("body is unit (empty tuple)");
    };
    assert!(unit.elems.is_empty());
    assert_eq!(ty_str(&unit.ty), "()");
}

// ── externs ──────────────────────────────────────────────────────

#[test]
fn extern_reference_carries_its_type() {
    let module = lower("extern fn sqrt(x: Float) -> Float", "Ffi");
    let [IrDecl::Extern(ext)] = module.declarations.as_slice() else {
        panic!("expected one extern, got {:?}", module.declarations);
    };
    assert_eq!(ext.name, "sqrt");
    assert_eq!(ty_str(&ext.ty), "Float \u{2192} Float");
    assert_eq!(ext.module, None);
}

// ── JSON serialization ───────────────────────────────────────────

#[test]
fn json_schema_is_stable() {
    // A tiny program pins the core schema exactly.
    let module = lower("fn answer() = 42", "Main");
    let json = module.to_json().expect("serialization succeeds");
    assert_eq!(
        json,
        r#"{"name":"Main","declarations":[{"kind":"Fn","name":"answer","params":[],"return_type":"Int","effect_row":"{}","body":{"kind":"Literal","value":{"Int":"42"},"type":"Int"}}]}"#
    );
}

#[test]
fn json_pretty_snapshot() {
    let module = lower(
        "type Option<a> = Some(a) | None\n\
         fn unwrap(opt: Option<Int>) -> Int = match opt { Some(x) -> x, None -> 0, }",
        "Opt",
    );
    insta::assert_snapshot!(module.to_json_pretty().expect("serialization succeeds"));
}

// ── handle blocks ────────────────────────────────────────────────

#[test]
fn handle_block_lowers_to_handle_node() {
    let module = lower(
        "effect Log\n\
         tool Repo : { x: Int } -> Int\n\
         fn audited(f: Int -> Int ! {Tool<Repo>}, logh: { x: Int } -> Int ! {Log}) -> Int ! {Log} =\n\
           handle { Tool<Repo> -> logh } in f(0)",
        "Handle",
    );
    let audited = fn_named(&module, "audited");
    let IrExpr::Handle(h) = &audited.body else {
        panic!("body should be a handle, got {:?}", audited.body);
    };
    // One arm handling `Tool<Repo>`, bound to the `logh` handler.
    assert_eq!(h.arms.len(), 1);
    assert_eq!(format!("{}", h.arms[0].effect), "Tool<Repo>");
    assert!(matches!(&h.arms[0].handler, IrExpr::Var(v) if v.name == "logh"));
    // The block's row: `Tool<Repo>` is handled away, the handler's `Log` joins.
    assert_eq!(format!("{}", h.effect_row), "{Log}");
    // The handled body is the call `f(0)`, and the block's value is its body's.
    assert!(matches!(h.body.as_ref(), IrExpr::App(_)));
    assert_eq!(ty_str(&h.result_type), "Int");
}

#[test]
fn handle_block_json_snapshot() {
    let module = lower(
        "effect Log\n\
         tool Repo : { x: Int } -> Int\n\
         fn audited(f: Int -> Int ! {Tool<Repo>}, logh: { x: Int } -> Int ! {Log}) -> Int ! {Log} =\n\
           handle { Tool<Repo> -> logh } in f(0)",
        "Handle",
    );
    insta::assert_snapshot!(module.to_json_pretty().expect("serialization succeeds"));
}

// ── install blocks ───────────────────────────────────────────────

#[test]
fn install_block_lowers_to_install_node() {
    let module = lower(
        "tool Repo : { x: Int } -> Int\n\
         fn demo(f: Int -> Int ! {Tool<Repo>}, h: { x: Int } -> Int) -> Int ! {Install, Tool<Repo>} =\n\
           install { Tool<Repo> -> h } in f(0)",
        "Install",
    );
    let demo = fn_named(&module, "demo");
    let IrExpr::Install(inst) = &demo.body else {
        panic!("body should be an install, got {:?}", demo.body);
    };
    // One arm installing `Tool<Repo>`, bound to the `h` handler.
    assert_eq!(inst.arms.len(), 1);
    assert_eq!(format!("{}", inst.arms[0].effect), "Tool<Repo>");
    assert!(matches!(&inst.arms[0].handler, IrExpr::Var(v) if v.name == "h"));
    // Nothing is handled away: the body's row plus `Install`.
    assert_eq!(format!("{}", inst.effect_row), "{Install, Tool<Repo>}");
    // The body is the call `f(0)`, and the block's value is its body's.
    assert!(matches!(inst.body.as_ref(), IrExpr::App(_)));
    assert_eq!(ty_str(&inst.result_type), "Int");
}

#[test]
fn install_block_json_snapshot() {
    let module = lower(
        "tool Repo : { x: Int } -> Int\n\
         fn demo(f: Int -> Int ! {Tool<Repo>}, h: { x: Int } -> Int) -> Int ! {Install, Tool<Repo>} =\n\
           install { Tool<Repo> -> h } in f(0)",
        "Install",
    );
    insta::assert_snapshot!(module.to_json_pretty().expect("serialization succeeds"));
}

// ── tool declarations ────────────────────────────────────────────

#[test]
fn tool_decl_lowers_to_tool_node() {
    let module = lower(
        "type Prompt = Prompt(String)\n\
         type Schema<t> = Schema(String)\n\
         type ParseError = ParseError(String)\n\
         tool LLMCall<t> : { prompt: Prompt, schema: Schema<t> } -> t ! {Exn<ParseError>}",
        "Tools",
    );
    let tool = module
        .declarations
        .iter()
        .find_map(|d| match d {
            IrDecl::Tool(t) => Some(t),
            _ => None,
        })
        .expect("tool declaration is present");
    assert_eq!(tool.name, "LLMCall");
    assert_eq!(tool.params, ["t"]);
    // Signature types render under the declared parameter name, and the
    // trailing row carries only the declared effects — the implicit
    // `Tool<LLMCall>` stays out.
    assert_eq!(ty_str(&tool.input), "{ prompt: Prompt, schema: Schema<t> }");
    assert_eq!(ty_str(&tool.output), "t");
    assert_eq!(format!("{}", tool.effect_row), "{Exn<ParseError>}");
}

// ── actor declarations ───────────────────────────────────────────

/// The counter actor plus a spawner, shared by the actor lowering tests.
const COUNTER: &str = "\
type St = St(Int)
actor Counter {
  state: St,
  message: Msg = | Inc | Quit,
  init: fn(s: St) ! {} = s,
  handle Inc, St(n) ! {} = Continue(St(n + 1)),
  handle Quit, st ! {} = Continue(st),
}
fn boot(s: St) -> Pid<Msg> ! {Spawn<Msg>} = spawn(Counter, s)";

#[test]
fn actor_lowers_to_actor_node() {
    let module = lower(COUNTER, "Actors");
    let actor = module
        .declarations
        .iter()
        .find_map(|d| match d {
            IrDecl::Actor(a) => Some(a),
            _ => None,
        })
        .expect("actor declaration is present");
    assert_eq!(actor.name, "Counter");
    assert_eq!(format!("{}", actor.state), "St");
    // The typed mailbox: the message sum type with its constructors.
    assert_eq!(actor.message.name, "Msg");
    assert_eq!(actor.message.constructors.len(), 2);
    assert_eq!(actor.message.constructors[0].name, "Inc");
    // Init: one `St` parameter, empty row, the parameter reference as body.
    assert_eq!(actor.init.params.len(), 1);
    assert_eq!(format!("{}", actor.init.params[0].ty), "St");
    assert!(actor.init.effect_row.is_empty());
    assert!(matches!(&actor.init.body, IrExpr::Var(v) if v.name == "s"));
    // Handlers carry the message and state patterns and their rows.
    assert_eq!(actor.handlers.len(), 2);
    assert!(matches!(&actor.handlers[0].message, IrPattern::Constructor(c) if c.name == "Inc"));
    assert!(matches!(&actor.handlers[0].state, IrPattern::Constructor(c) if c.name == "St"));
    assert!(matches!(&actor.handlers[1].state, IrPattern::Bind(b) if b.name == "st"));
    // No declared summary: the empty row.
    assert!(actor.effect_row.is_empty());
}

#[test]
fn spawn_lowers_to_spawn_node() {
    let module = lower(COUNTER, "Actors");
    let boot = fn_named(&module, "boot");
    let IrExpr::Spawn(spawn) = &boot.body else {
        panic!("body should be a spawn, got {:?}", boot.body);
    };
    assert_eq!(spawn.actor, "Counter");
    assert_eq!(spawn.args.len(), 1);
    assert!(matches!(&spawn.args[0], IrExpr::Var(v) if v.name == "s"));
    // The typed reference: `Pid<Msg>` for the actor's message type.
    assert_eq!(ty_str(&spawn.result_type), "Pid<Msg>");
    // The spawner's declared row carries the spawn effect.
    assert_eq!(format!("{}", boot.effect_row), "{Spawn<Msg>}");
}

#[test]
fn actor_json_snapshot() {
    let module = lower(COUNTER, "Actors");
    insta::assert_snapshot!(module.to_json_pretty().expect("serialization succeeds"));
}

// ── messaging primitives ─────────────────────────────────────────

/// A request/reply counter plus senders, shared by the messaging tests.
const MESSAGING: &str = "\
type Status = Status(Int)
type St = St(Int)
actor Counter {
  state: St,
  message: Msg = | Inc | Get(ReplyTo<Status>),
  init: fn(s: St) ! {} = s,
  handle Inc, St(n) ! {} = Continue(St(n + 1)),
  handle Get(r), St(n) ! {Send<Status>} = let sent = reply(r, Status(n)) in Continue(St(n)),
} ! {Send<Status>}
fn poke(p: Pid<Msg>) ! {Send<Msg>} = send(p, Inc)
fn query(p: Pid<Msg>) -> Status ! {Send<Msg>, Await<Status>} = request(p, Get)";

#[test]
fn send_lowers_to_send_node() {
    let module = lower(MESSAGING, "Messaging");
    let poke = fn_named(&module, "poke");
    let IrExpr::Send(send) = &poke.body else {
        panic!("body should be a send, got {:?}", poke.body);
    };
    assert!(matches!(send.pid.as_ref(), IrExpr::Var(v) if v.name == "p"));
    assert!(matches!(send.message.as_ref(), IrExpr::Constructor(c) if c.name == "Inc"));
    // Fire-and-forget: unit-valued.
    assert_eq!(ty_str(&send.result_type), "()");
    assert_eq!(format!("{}", poke.effect_row), "{Send<Msg>}");
}

#[test]
fn request_lowers_to_request_node() {
    let module = lower(MESSAGING, "Messaging");
    let query = fn_named(&module, "query");
    let IrExpr::Request(request) = &query.body else {
        panic!("body should be a request, got {:?}", query.body);
    };
    assert!(matches!(request.pid.as_ref(), IrExpr::Var(v) if v.name == "p"));
    assert!(matches!(request.message_fn.as_ref(), IrExpr::Constructor(c) if c.name == "Get"));
    // The expression's type is the reply type, not the message type.
    assert_eq!(ty_str(&request.result_type), "Status");
    assert_eq!(
        format!("{}", query.effect_row),
        "{Await<Status>, Send<Msg>}"
    );
}

#[test]
fn reply_lowers_to_reply_node() {
    let module = lower(MESSAGING, "Messaging");
    let actor = module
        .declarations
        .iter()
        .find_map(|d| match d {
            IrDecl::Actor(a) => Some(a),
            _ => None,
        })
        .expect("actor declaration is present");
    let IrExpr::Let(le) = &actor.handlers[1].body else {
        panic!("handler body should be a let, got {:?}", actor.handlers[1]);
    };
    let IrExpr::Reply(reply) = le.value.as_ref() else {
        panic!("bound value should be a reply, got {:?}", le.value);
    };
    assert!(matches!(reply.reply_to.as_ref(), IrExpr::Var(v) if v.name == "r"));
    assert!(matches!(reply.value.as_ref(), IrExpr::Constructor(c) if c.name == "Status"));
    assert_eq!(ty_str(&reply.result_type), "()");
    assert_eq!(
        format!("{}", actor.handlers[1].effect_row),
        "{Send<Status>}"
    );
}

#[test]
fn messaging_json_snapshot() {
    let module = lower(MESSAGING, "Messaging");
    insta::assert_snapshot!(module.to_json_pretty().expect("serialization succeeds"));
}

// ── supervisor declarations ──────────────────────────────────────

/// A planner actor supervised by a `one_for_one` supervisor, shared by the
/// supervisor lowering tests.
const SUPERVISED: &str = "\
type Path = Path(String)
type St = St(Int)
tool ReadRepo : { path: Path } -> St
fn planner_config() -> St = St(0)
actor Planner {
  state: St,
  message: Msg = | Plan(Path) | Quit,
  init: fn(c: St) ! {} = c,
  handle Plan(p), st ! {Tool<ReadRepo>} = Continue(read_repo({ path: p })),
  handle Quit, st ! {} = Continue(st),
} ! {Tool<ReadRepo>}
supervisor PlannerSup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: planner, actor: Planner, start_args: planner_config(), restart: permanent },
  ]
}";

#[test]
fn supervisor_lowers_to_supervisor_node() {
    let module = lower(SUPERVISED, "Sup");
    let sup = module
        .declarations
        .iter()
        .find_map(|d| match d {
            IrDecl::Supervisor(s) => Some(s),
            _ => None,
        })
        .expect("supervisor declaration is present");
    assert_eq!(sup.name, "PlannerSup");
    assert_eq!(sup.strategy, "one_for_one");
    assert_eq!(sup.intensity, 5);
    assert_eq!(sup.period, 60);
    // The derived row is the child actor's per-actor summary.
    assert_eq!(format!("{}", sup.effect_row), "{Tool<ReadRepo>}");
    // One child: its identifiers verbatim, its start argument the lowered
    // config call.
    assert_eq!(sup.children.len(), 1);
    let child = &sup.children[0];
    assert_eq!(child.id, "planner");
    assert_eq!(child.actor, "Planner");
    assert_eq!(child.restart, "permanent");
    let IrExpr::App(app) = &child.start_args else {
        panic!("start_args should be a call, got {:?}", child.start_args);
    };
    assert!(matches!(app.func.as_ref(), IrExpr::Var(v) if v.name == "planner_config"));
    assert!(app.args.is_empty());
}

#[test]
fn supervisor_json_snapshot() {
    let module = lower(SUPERVISED, "Sup");
    insta::assert_snapshot!(module.to_json_pretty().expect("serialization succeeds"));
}

// ── supervise and child expressions ──────────────────────────────

/// The supervised fixture plus the runtime-surface consumers.
fn supervised_with_consumers() -> String {
    format!(
        "{SUPERVISED}\n\
         fn boot() ! {{Supervise}} = supervise(PlannerSup)\n\
         fn planner() -> Pid<Msg> ! {{}} = child(PlannerSup, planner)"
    )
}

#[test]
fn supervise_lowers_to_supervise_node() {
    let module = lower(&supervised_with_consumers(), "Sup");
    let boot = fn_named(&module, "boot");
    let IrExpr::Supervise(supervise) = &boot.body else {
        panic!("body should be a supervise, got {:?}", boot.body);
    };
    assert_eq!(supervise.supervisor, "PlannerSup");
    assert_eq!(ty_str(&supervise.result_type), "()");
    // The booter's declared row carries the bare supervise effect.
    assert_eq!(format!("{}", boot.effect_row), "{Supervise}");
}

#[test]
fn stand_lowers_to_stand_node() {
    let module = lower(
        &format!("{SUPERVISED}\nfn serve() ! {{Stand}} = stand()"),
        "Sup",
    );
    let serve = fn_named(&module, "serve");
    let IrExpr::Stand(stand) = &serve.body else {
        panic!("body should be a stand, got {:?}", serve.body);
    };
    assert_eq!(ty_str(&stand.result_type), "()");
    assert_eq!(format!("{}", serve.effect_row), "{Stand}");
}

#[test]
fn child_lowers_to_child_node() {
    let module = lower(&supervised_with_consumers(), "Sup");
    let planner = fn_named(&module, "planner");
    let IrExpr::Child(child) = &planner.body else {
        panic!("body should be a child lookup, got {:?}", planner.body);
    };
    assert_eq!(child.supervisor, "PlannerSup");
    assert_eq!(child.child_id, "planner");
    // The typed reference: `Pid<Msg>` for the child actor's message type.
    assert_eq!(ty_str(&child.result_type), "Pid<Msg>");
    // The lookup is effect-free.
    assert!(planner.effect_row.is_empty());
}

// ── crash primitive ──────────────────────────────────────────────

#[test]
fn crash_lowers_to_crash_node() {
    let module = lower(r#"fn boom() -> Int = crash!("nope")"#, "Crash");
    let boom = only_fn(&module);
    let IrExpr::Crash(crash) = &boom.body else {
        panic!("body should be a crash, got {:?}", boom.body);
    };
    // The message is the string literal, kept verbatim (quotes and all).
    let IrExpr::Literal(lit) = crash.message.as_ref() else {
        panic!("message should be a literal, got {:?}", crash.message);
    };
    assert_eq!(lit.value, LiteralValue::Str(Box::from("\"nope\"")));
    // The recorded type is the demanded context type, not a bottom type.
    assert_eq!(ty_str(&crash.result_type), "Int");
    // Crashing is not an effect: the function's row stays empty.
    assert!(boom.effect_row.is_empty());
}

#[test]
fn panic_lowers_to_the_same_crash_node() {
    // `panic!` is a surface alias; it lowers to the identical `IrExpr::Crash`.
    let module = lower(r#"fn boom() -> Int = panic!("nope")"#, "Crash");
    let boom = only_fn(&module);
    assert!(
        matches!(&boom.body, IrExpr::Crash(_)),
        "panic! should lower to a crash node, got {:?}",
        boom.body
    );
}

#[test]
fn crash_json_snapshot() {
    let module = lower(r#"fn boom() -> Int = crash!("nope")"#, "Crash");
    insta::assert_snapshot!(module.to_json_pretty().expect("serialization succeeds"));
}

// ── declaration spans ────────────────────────────────────────────

#[test]
fn declarations_carry_source_lines() {
    let module = lower(
        "type Flag = On | Off\n\
         \n\
         fn pick(f: Flag) -> Int =\n\
           match f { On -> 1, Off -> 0, }\n\
         extern fn sqrt(x: Float) -> Float",
        "Spans",
    );
    let lines: Vec<u32> = module
        .declarations
        .iter()
        .map(|d| match d {
            IrDecl::Fn(f) => f.span.line,
            IrDecl::Type(t) => t.span.line,
            IrDecl::Extern(e) => e.span.line,
            other => panic!("unexpected declaration {other:?}"),
        })
        .collect();
    assert_eq!(lines, [1, 3, 5]);
}

// ── lambda effect rows ───────────────────────────────────────────

#[test]
fn lambda_carries_its_effect_row() {
    // The outer lambda's own function type carries `{Log}`; the pure inner
    // binding carries an empty (or open, once generalised) row distinct from it.
    let module = lower(
        "effect Log\n\
         fn wrap(run: Int -> Int ! {Log}) -> (Int -> Int ! {Log}) ! {} =\n\
           \\x -> run(x)",
        "Rows",
    );
    let wrap = only_fn(&module);
    let IrExpr::Lambda(lambda) = &wrap.body else {
        panic!("body should be a lambda, got {:?}", wrap.body);
    };
    assert_eq!(format!("{}", lambda.effect_row), "{Log}");
}

/// A self-driving actor: `clock`, `self`, and `schedule` lower to their own
/// nodes with the checked types, `request` carries its optional timeout, and
/// the rows record `Clock` and `Schedule<Msg>`.
const TIMED: &str = "\
type Status = Status(Int)
type Cfg = Cfg(Clock, Int)
actor Heart {
  state: Cfg,
  message: HeartMsg = | Beat | Get(ReplyTo<Status>),
  init: fn(c: Cfg) ! {} = c,
  handle Beat, Cfg(clock, period) ! {Schedule<HeartMsg>} =
    let next = schedule(clock, self(), Beat, period) in Continue(Cfg(clock, period)),
  handle Get(r), st ! {Send<Status>} = let sent = reply(r, Status(1)) in Continue(st),
} ! {Schedule<HeartMsg>, Send<Status>}
supervisor HeartSup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: heart, actor: Heart, start_args: Cfg(clock(), 1000), restart: permanent },
  ]
}
fn acquire() -> Clock ! {Clock} = clock()
fn patient(p: Pid<HeartMsg>) -> Status ! {Send<HeartMsg>, Await<Status>} = request(p, Get, 60000)
fn prompt(p: Pid<HeartMsg>) -> Status ! {Send<HeartMsg>, Await<Status>} = request(p, Get)
";

#[test]
fn clock_lowers_to_clock_node() {
    let module = lower(TIMED, "Timed");
    let acquire = fn_named(&module, "acquire");
    let IrExpr::Clock(clock) = &acquire.body else {
        panic!("body should be a clock, got {:?}", acquire.body);
    };
    assert_eq!(ty_str(&clock.result_type), "Clock");
    assert_eq!(format!("{}", acquire.effect_row), "{Clock}");
}

#[test]
fn request_carries_optional_timeout() {
    let module = lower(TIMED, "Timed");
    let patient = fn_named(&module, "patient");
    let IrExpr::Request(request) = &patient.body else {
        panic!("body should be a request, got {:?}", patient.body);
    };
    let Some(IrExpr::Literal(timeout)) = request.timeout.as_deref() else {
        panic!("the timeout should be a literal, got {:?}", request.timeout);
    };
    assert_eq!(timeout.value, LiteralValue::Int(Box::from("60000")));
    // The timeout is not an effect: the row is the plain request row.
    assert_eq!(
        format!("{}", patient.effect_row),
        "{Await<Status>, Send<HeartMsg>}"
    );
    let prompt = fn_named(&module, "prompt");
    let IrExpr::Request(request) = &prompt.body else {
        panic!("body should be a request, got {:?}", prompt.body);
    };
    assert!(request.timeout.is_none());
}

#[test]
fn schedule_and_self_lower_inside_actor() {
    let module = lower(TIMED, "Timed");
    let actor = module
        .declarations
        .iter()
        .find_map(|d| match d {
            IrDecl::Actor(a) => Some(a),
            _ => None,
        })
        .expect("actor declaration is present");
    let beat = actor
        .handlers
        .iter()
        .find(|h| matches!(&h.message, IrPattern::Constructor(c) if c.name == "Beat"))
        .expect("the Beat handler is present");
    let IrExpr::Let(binding) = &beat.body else {
        panic!("handler body should be a let, got {:?}", beat.body);
    };
    let IrExpr::Schedule(schedule) = binding.value.as_ref() else {
        panic!(
            "the bound value should be a schedule, got {:?}",
            binding.value
        );
    };
    assert_eq!(ty_str(&schedule.result_type), "()");
    let IrExpr::SelfRef(this) = schedule.pid.as_ref() else {
        panic!("the destination should be self, got {:?}", schedule.pid);
    };
    assert_eq!(ty_str(&this.result_type), "Pid<HeartMsg>");
    assert_eq!(format!("{}", beat.effect_row), "{Schedule<HeartMsg>}");
    // The supervisor's derived row records the clock its child spec acquires.
    let sup = module
        .declarations
        .iter()
        .find_map(|d| match d {
            IrDecl::Supervisor(s) => Some(s),
            _ => None,
        })
        .expect("supervisor declaration is present");
    assert_eq!(
        format!("{}", sup.effect_row),
        "{Clock, Schedule<HeartMsg>, Send<Status>}"
    );
    assert!(matches!(
        &sup.children[0].start_args,
        IrExpr::Constructor(c) if matches!(c.args.as_slice(), [IrExpr::Clock(_), _])
    ));
}

// ── imported functions qualify to their defining module ─────────

/// An unqualified imported function lowers as the qualified `Mod.member`
/// variable remote calls already lower through — in call and value position
/// alike — while a use under a local shadow stays bare.
#[test]
fn imported_function_uses_qualify_to_the_defining_module() {
    use hird_check::ModuleName;

    let sources = [
        ("Lib", "module Lib\npub fn double(x: Int) -> Int = x + x"),
        (
            "App",
            "module App\nuse Lib.{double}\n\
             fn call(x: Int) -> Int = double(x)\n\
             fn value(x: Int) -> Int = let f = double in f(x)\n\
             fn shadowed(x: Int) -> Int = let double = \\y -> y in double(x)",
        ),
    ];
    let program: Vec<(ModuleName, SourceFile)> = sources
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
    let mut checked = hird_check::check_program(&program);
    let app = checked
        .modules
        .remove(&ModuleName::new("App"))
        .expect("App was checked");
    assert!(!app.has_errors(), "type errors: {:?}", app.diagnostics);
    let module = lower_module(&program[1].1, &app, "App");

    let var_name = |body: &IrExpr| match body {
        IrExpr::App(app) => match app.func.as_ref() {
            IrExpr::Var(v) => v.name.clone(),
            other => panic!("expected a variable callee, got {other:?}"),
        },
        other => panic!("expected an application body, got {other:?}"),
    };
    assert_eq!(var_name(&fn_named(&module, "call").body), "Lib.double");
    let IrExpr::Let(bound) = &fn_named(&module, "value").body else {
        panic!("expected a let body");
    };
    assert!(
        matches!(bound.value.as_ref(), IrExpr::Var(v) if v.name == "Lib.double"),
        "bound value: {:?}",
        bound.value
    );
    // The shadowing let binds a lambda; the use under it stays bare.
    let IrExpr::Let(shadow) = &fn_named(&module, "shadowed").body else {
        panic!("expected a let body");
    };
    assert_eq!(var_name(&shadow.body), "double");
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

use hird_ast::{
    AstNode, Constructor, Decl, Expr, FnDecl, Pattern, SourceFile, TypeDecl, TypeExpr, UseDecl,
};

fn file(src: &str) -> SourceFile {
    let parsed = hird_parse::parse(src, 0);
    assert!(parsed.is_ok(), "parse errors: {:?}", parsed.diagnostics());
    SourceFile::cast(parsed.syntax().clone()).expect("source file root")
}

fn first_fn(file: &SourceFile) -> FnDecl {
    file.declarations()
        .find_map(|d| match d {
            Decl::Fn(f) => Some(f),
            _ => None,
        })
        .expect("a fn declaration")
}

fn first_type(file: &SourceFile) -> TypeDecl {
    file.declarations()
        .find_map(|d| match d {
            Decl::Type(t) => Some(t),
            _ => None,
        })
        .expect("a type declaration")
}

fn first_use(file: &SourceFile) -> UseDecl {
    file.declarations()
        .find_map(|d| match d {
            Decl::Use(u) => Some(u),
            _ => None,
        })
        .expect("a use declaration")
}

/// Parse `fn f() = <expr>` and return the body expression.
fn body(expr_src: &str) -> Expr {
    let src = format!("fn f() = {expr_src}");
    let file = file(&src);
    first_fn(&file).body().expect("a fn body")
}

/// Collect an iterator of borrowed strings into owned ones.
fn owned<'a>(it: impl Iterator<Item = &'a str>) -> Vec<String> {
    it.map(str::to_owned).collect()
}

// ── declarations ────────────────────────────────────────────────

#[test]
fn source_file_declarations() {
    let file = file(
        "\
module Planner

use Actors.Base

effect Log

type Result<A> = Ok(A) | Err(String)

pub fn identity(x: Int) -> Int = x

extern fn print(s: String) -> Unit",
    );

    let module_name = file.module().and_then(|m| m.name().map(str::to_owned));
    assert_eq!(module_name.as_deref(), Some("Planner"));

    let kinds: Vec<&str> = file
        .declarations()
        .map(|d| match d {
            Decl::Module(_) => "module",
            Decl::Use(_) => "use",
            Decl::Effect(_) => "effect",
            Decl::Type(_) => "type",
            Decl::Fn(_) => "fn",
            Decl::Extern(_) => "extern",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["module", "use", "effect", "type", "fn", "extern"]);
}

#[test]
fn use_decl_whole_module() {
    let u = first_use(&file("use Ets"));
    assert_eq!(owned(u.path().expect("path").segments()), ["Ets"]);
    assert_eq!(u.alias(), None);
    assert!(owned(u.selected()).is_empty());
}

#[test]
fn use_decl_path_and_alias() {
    let u = first_use(&file("use Foo.Bar.Baz as B"));
    assert_eq!(
        owned(u.path().expect("path").segments()),
        ["Foo", "Bar", "Baz"]
    );
    assert_eq!(u.alias(), Some("B"));
    assert!(owned(u.selected()).is_empty());
}

#[test]
fn use_decl_selective() {
    // The path segment stays in `path()`; the brace members project through
    // `selected()`. A selective import carries no alias.
    let u = first_use(&file("use Ets.{Table, lookup}"));
    assert_eq!(owned(u.path().expect("path").segments()), ["Ets"]);
    assert_eq!(owned(u.selected()), ["Table", "lookup"]);
    assert_eq!(u.alias(), None);
}

#[test]
fn fn_decl_basics() {
    let file = file("fn add(x: Int, y: Int) -> Int = x");
    let f = first_fn(&file);
    assert_eq!(f.name(), Some("add"));
    assert!(!f.is_pub());
    // `params()` yields owned nodes, so copy each name before the node drops.
    let params: Vec<String> = f
        .params()
        .filter_map(|p| p.name().map(str::to_owned))
        .collect();
    assert_eq!(params, ["x", "y"]);
    assert!(matches!(f.body(), Some(Expr::Name(_))));
}

#[test]
fn fn_pub_visibility() {
    let file = file("pub fn f() = 0");
    assert!(first_fn(&file).is_pub());
}

#[test]
fn type_decl_constructors() {
    let file = file("type Option<A> = Some(A) | None");
    let ty = file
        .declarations()
        .find_map(|d| match d {
            Decl::Type(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(ty.name(), Some("Option"));
    let ctors: Vec<String> = ty
        .constructors()
        .filter_map(|c| c.name().map(str::to_owned))
        .collect();
    assert_eq!(ctors, ["Some", "None"]);
}

#[test]
fn type_decl_opacity() {
    // The three visibility levels: private, transparent (`pub`), and opaque
    // (`pub opaque`). `file` asserts a clean parse, so each form is well-formed.
    let t = first_type(&file("type Foo = Bar(Int)"));
    assert!(!t.is_pub());
    assert!(!t.is_opaque());

    let t = first_type(&file("pub type Foo = Bar(Int)"));
    assert!(t.is_pub());
    assert!(!t.is_opaque());

    let t = first_type(&file("pub opaque type Foo = Bar(Int)"));
    assert!(t.is_pub());
    assert!(t.is_opaque());
    // Name and constructors project unchanged through the opaque modifier.
    assert_eq!(t.name(), Some("Foo"));
    let ctors: Vec<String> = t
        .constructors()
        .filter_map(|c| c.name().map(str::to_owned))
        .collect();
    assert_eq!(ctors, ["Bar"]);
}

#[test]
fn actor_supervisor_names() {
    let actor_file = file("actor Planner { state: PlannerState }");
    let actor = actor_file
        .declarations()
        .find_map(|d| match d {
            Decl::Actor(a) => Some(a),
            _ => None,
        })
        .unwrap();
    assert_eq!(actor.name(), Some("Planner"));
    assert!(!actor.is_pub());

    let sup_file = file("supervisor PlannerSup { strategy: one_for_one }");
    let sup = sup_file
        .declarations()
        .find_map(|d| match d {
            Decl::Supervisor(s) => Some(s),
            _ => None,
        })
        .unwrap();
    assert_eq!(sup.name(), Some("PlannerSup"));
}

// ── expressions ─────────────────────────────────────────────────

#[test]
fn atomic_bodies_are_tokens() {
    // The parser does not wrap atoms in nodes, so a bare literal/ident body is
    // projected as a token-backed Expr variant rather than being lost.
    let Expr::Literal(lit) = body("42") else {
        panic!("expected literal");
    };
    assert_eq!(lit.text(), "42");

    let Expr::Name(name) = body("x") else {
        panic!("expected name");
    };
    assert_eq!(name.text(), "x");
}

#[test]
fn let_expr_parts() {
    let Expr::Let(e) = body("let x = 42 in x") else {
        panic!("expected let");
    };
    assert_eq!(e.name(), Some("x"));
    assert!(matches!(e.value(), Some(Expr::Literal(_))));
    assert!(matches!(e.body(), Some(Expr::Name(_))));
}

#[test]
fn lambda_expr_parts() {
    let Expr::Lambda(e) = body("\\x y -> x") else {
        panic!("expected lambda");
    };
    assert_eq!(owned(e.param_names()), ["x", "y"]);
    assert!(matches!(e.body(), Some(Expr::Name(_))));
}

#[test]
fn if_expr_parts() {
    let Expr::If(e) = body("if c then a else b") else {
        panic!("expected if");
    };
    assert!(matches!(e.condition(), Some(Expr::Name(_))));
    assert!(matches!(e.then_branch(), Some(Expr::Name(_))));
    assert!(matches!(e.else_branch(), Some(Expr::Name(_))));
}

#[test]
fn match_expr_parts() {
    let Expr::Match(e) = body("match x { Foo -> 1, Bar -> 2 }") else {
        panic!("expected match");
    };
    assert!(matches!(e.scrutinee(), Some(Expr::Name(_))));
    let arm_bodies: Vec<String> = e
        .arms()
        .filter_map(|a| match a.body() {
            Some(Expr::Literal(l)) => Some(l.text().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(arm_bodies, ["1", "2"]);
}

#[test]
fn handle_block_parts() {
    let Expr::Handle(e) = body("handle { Log -> a, Tool<ReadRepo> -> b } in m") else {
        panic!("expected handle");
    };
    assert_eq!(e.arms().count(), 2);
    assert!(matches!(e.body(), Some(Expr::Name(_))));
    let arms: Vec<_> = e.arms().collect();
    // A bare effect head is a name; a parametric one is a type application.
    assert!(matches!(arms[0].effect(), Some(TypeExpr::Name(_))));
    assert!(matches!(arms[0].handler(), Some(Expr::Name(_))));
    assert!(matches!(arms[1].effect(), Some(TypeExpr::App(_))));
}

#[test]
fn bin_op_precedence() {
    // `x + y * z` parses as `x + (y * z)`.
    let Expr::BinOp(add) = body("x + y * z") else {
        panic!("expected binop");
    };
    assert_eq!(add.op(), Some("+"));
    assert!(matches!(add.lhs(), Some(Expr::Name(_))));
    let Some(Expr::BinOp(mul)) = add.rhs() else {
        panic!("expected nested binop on the right");
    };
    assert_eq!(mul.op(), Some("*"));
}

#[test]
fn bin_op_logical_precedence() {
    // `a && b || c` parses as `(a && b) || c` — `||` binds looser than `&&`.
    let Expr::BinOp(or) = body("a && b || c") else {
        panic!("expected binop");
    };
    assert_eq!(or.op(), Some("||"));
    assert!(matches!(or.rhs(), Some(Expr::Name(_))));
    let Some(Expr::BinOp(and)) = or.lhs() else {
        panic!("expected nested binop on the left");
    };
    assert_eq!(and.op(), Some("&&"));
}

#[test]
fn app_expr_is_left_nested() {
    // `f x y` parses as `(f x) y`.
    let Expr::App(outer) = body("f x y") else {
        panic!("expected application");
    };
    assert!(matches!(outer.function(), Some(Expr::App(_))));
    assert!(matches!(outer.argument(), Some(Expr::Name(_))));
}

#[test]
fn field_expr_parts() {
    // `a.b.c` parses as `(a.b).c`.
    let Expr::Field(outer) = body("a.b.c") else {
        panic!("expected field access");
    };
    assert_eq!(outer.field(), Some("c"));
    assert!(matches!(outer.receiver(), Some(Expr::Field(_))));
}

#[test]
fn record_lit_fields_and_keyword_key() {
    let Expr::Record(rec) = body("{ actor: Planner, count: 5 }") else {
        panic!("expected record");
    };
    let names: Vec<String> = rec
        .fields()
        .filter_map(|f| f.name().map(str::to_owned))
        .collect();
    assert_eq!(names, ["actor", "count"]);
    let first = rec.fields().next().unwrap();
    assert!(matches!(first.value(), Some(Expr::Name(_))));
}

#[test]
fn tuple_list_paren() {
    let Expr::Tuple(t) = body("(a, b, c)") else {
        panic!("expected tuple");
    };
    assert_eq!(t.elements().count(), 3);

    let Expr::List(l) = body("[1, 2]") else {
        panic!("expected list");
    };
    assert_eq!(l.elements().count(), 2);

    let Expr::Paren(p) = body("(x)") else {
        panic!("expected paren");
    };
    assert!(matches!(p.inner(), Some(Expr::Name(_))));
}

// ── partial-AST recovery ────────────────────────────────────────

/// Parse `src` into a `SourceFile` without requiring a clean parse. Used by the
/// recovery tests, where diagnostics are expected but the good declarations
/// must still project.
fn recovered(src: &str) -> SourceFile {
    let parsed = hird_parse::parse(src, 0);
    assert!(!parsed.is_ok(), "expected diagnostics for: {src}");
    SourceFile::cast(parsed.syntax().clone()).expect("source file root")
}

#[test]
fn recovery_isolates_garbage_between_declarations() {
    // The stray `99 88` is wrapped in an error node, not a declaration, so it
    // is skipped by `declarations()`; the effect and function around it project
    // normally.
    let file = recovered("effect Alpha\n99 88\nfn beta() = 1");

    let kinds: Vec<&str> = file
        .declarations()
        .map(|d| match d {
            Decl::Effect(_) => "effect",
            Decl::Fn(_) => "fn",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["effect", "fn"]);

    let names: Vec<String> = file
        .declarations()
        .filter_map(|d| match d {
            Decl::Effect(e) => e.name().map(str::to_owned),
            Decl::Fn(f) => f.name().map(str::to_owned),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["Alpha", "beta"]);
}

#[test]
fn recovery_keeps_neighbours_of_malformed_declaration() {
    // The middle function has a malformed parameter type (`x:` with no type),
    // yet it still projects — name, parameter, and body — alongside its
    // well-formed neighbours.
    let file = recovered("fn alpha() = 1\nfn broken(x: ) = 2\nfn beta() = 3");

    let fns: Vec<FnDecl> = file
        .declarations()
        .filter_map(|d| match d {
            Decl::Fn(f) => Some(f),
            _ => None,
        })
        .collect();
    let names: Vec<String> = fns
        .iter()
        .filter_map(|f| f.name().map(str::to_owned))
        .collect();
    assert_eq!(names, ["alpha", "broken", "beta"]);

    let params: Vec<String> = fns[1]
        .params()
        .filter_map(|p| p.name().map(str::to_owned))
        .collect();
    assert_eq!(params, ["x"]);
    assert!(matches!(fns[1].body(), Some(Expr::Literal(_))));
}

// ── type expressions ────────────────────────────────────────────

/// Parse `fn f(x: <ty_src>) = 0` and return the parameter's projected type.
fn param_type(ty_src: &str) -> TypeExpr {
    let src = format!("fn f(x: {ty_src}) = 0");
    let file = file(&src);
    first_fn(&file)
        .params()
        .next()
        .expect("a param")
        .ty()
        .expect("a param type")
}

/// The named field types of a constructor (non-name fields ignored).
fn ctor_field_names(c: &Constructor) -> Vec<String> {
    c.fields()
        .filter_map(|f| match f {
            TypeExpr::Name(n) => Some(n.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn type_name_and_variable() {
    // A bare identifier projects as a name, whether a named type or a type
    // variable — the two are indistinguishable at this layer.
    let TypeExpr::Name(named) = param_type("Int") else {
        panic!("expected name type");
    };
    assert_eq!(named.text(), "Int");

    let TypeExpr::Name(var) = param_type("a") else {
        panic!("expected name type");
    };
    assert_eq!(var.text(), "a");
}

#[test]
fn type_applied() {
    let TypeExpr::App(app) = param_type("List<Int>") else {
        panic!("expected applied type");
    };
    assert_eq!(app.name(), Some("List"));
    let args: Vec<String> = app
        .args()
        .filter_map(|a| match a {
            TypeExpr::Name(n) => Some(n.text().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(args, ["Int"]);

    // Several arguments.
    let TypeExpr::App(table) = param_type("Table<UserId, User, Read>") else {
        panic!("expected applied type");
    };
    assert_eq!(table.args().count(), 3);
}

#[test]
fn type_function_is_flat_curried() {
    // `a → b → c` flattens to operands [a, b, c]: params [a, b], result c.
    let TypeExpr::Fn(f) = param_type("a -> b -> c") else {
        panic!("expected function type");
    };
    let params: Vec<String> = f
        .params()
        .filter_map(|t| match t {
            TypeExpr::Name(n) => Some(n.text().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(params, ["a", "b"]);
    let Some(TypeExpr::Name(ret)) = f.return_type() else {
        panic!("expected name result");
    };
    assert_eq!(ret.text(), "c");
}

#[test]
fn type_tuple_and_paren() {
    let TypeExpr::Tuple(t) = param_type("(A, B)") else {
        panic!("expected tuple type");
    };
    assert_eq!(t.elements().count(), 2);

    // `()` is the unit (empty tuple) type.
    let TypeExpr::Tuple(unit) = param_type("()") else {
        panic!("expected unit type");
    };
    assert_eq!(unit.elements().count(), 0);

    // A single parenthesised type is a paren, not a one-tuple.
    let TypeExpr::Paren(p) = param_type("(T)") else {
        panic!("expected paren type");
    };
    assert!(matches!(p.inner(), Some(TypeExpr::Name(_))));
}

#[test]
fn type_record() {
    // Field names project in source order; a keyword spelling (`tool`) is a
    // valid field name.
    let TypeExpr::Record(r) = param_type("{ x: Int, tool: String }") else {
        panic!("expected record type");
    };
    let names: Vec<String> = r
        .fields()
        .filter_map(|f| f.name().map(str::to_owned))
        .collect();
    assert_eq!(names, ["x", "tool"]);
    assert!(
        r.fields()
            .all(|f| matches!(f.ty(), Some(TypeExpr::Name(_))))
    );

    let TypeExpr::Record(empty) = param_type("{}") else {
        panic!("expected record type");
    };
    assert_eq!(empty.fields().count(), 0);
}

#[test]
fn tool_decl_projects() {
    let file =
        file("tool LLMCall<t> : { prompt: Prompt, schema: Schema<t> } -> t ! {Exn<ParseError>}");
    let tool = file
        .declarations()
        .find_map(|d| match d {
            Decl::Tool(t) => Some(t),
            _ => None,
        })
        .expect("a tool declaration");
    assert_eq!(tool.name(), Some("LLMCall"));
    assert_eq!(owned(tool.type_params()), ["t"]);
    assert!(matches!(tool.input(), Some(TypeExpr::Record(_))));
    assert!(matches!(tool.output(), Some(TypeExpr::Name(_))));
    assert!(tool.effect_ann().is_some());
}

#[test]
fn fn_signature_types() {
    let file = file("fn add(x: Int, y: List<a>) -> Int = x");
    let f = first_fn(&file);

    let kinds: Vec<&str> = f
        .params()
        .map(|p| match p.ty() {
            Some(TypeExpr::Name(_)) => "name",
            Some(TypeExpr::App(_)) => "app",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["name", "app"]);

    let Some(TypeExpr::Name(ret)) = f.return_type() else {
        panic!("expected return type");
    };
    assert_eq!(ret.text(), "Int");
}

#[test]
fn extern_signature_types() {
    let file = file("extern fn print(s: String) -> Unit");
    let ext = file
        .declarations()
        .find_map(|d| match d {
            Decl::Extern(e) => Some(e),
            _ => None,
        })
        .unwrap();

    let Some(TypeExpr::Name(param)) = ext.params().next().and_then(|p| p.ty()) else {
        panic!("expected param type");
    };
    assert_eq!(param.text(), "String");

    let Some(TypeExpr::Name(ret)) = ext.return_type() else {
        panic!("expected return type");
    };
    assert_eq!(ret.text(), "Unit");
}

#[test]
fn type_decl_params_and_constructor_fields() {
    let file = file("type Result<A, B> = Ok(A) | Err(B, String) | Pending");
    let ty = file
        .declarations()
        .find_map(|d| match d {
            Decl::Type(t) => Some(t),
            _ => None,
        })
        .unwrap();

    assert_eq!(owned(ty.type_params()), ["A", "B"]);

    let ctors: Vec<_> = ty.constructors().collect();
    assert_eq!(ctor_field_names(&ctors[0]), ["A"]);
    assert_eq!(ctor_field_names(&ctors[1]), ["B", "String"]);
    assert!(ctor_field_names(&ctors[2]).is_empty()); // `Pending` is nullary
}

#[test]
fn let_annotation() {
    let Expr::Let(annotated) = body("let x: Int = 42 in x") else {
        panic!("expected let");
    };
    let Some(TypeExpr::Name(ann)) = annotated.annotation() else {
        panic!("expected annotation");
    };
    assert_eq!(ann.text(), "Int");

    let Expr::Let(bare) = body("let y = 1 in y") else {
        panic!("expected let");
    };
    assert!(bare.annotation().is_none());
}

// ── patterns ─────────────────────────────────────────────────────

/// Parse `fn f() = match m { <pat_src> → 0 }` and return the arm's pattern.
fn arm_pattern(pat_src: &str) -> Pattern {
    let src = format!("fn f() = match m {{ {pat_src} -> 0 }}");
    let file = file(&src);
    let Some(Expr::Match(m)) = first_fn(&file).body() else {
        panic!("expected match body");
    };
    m.arms()
        .next()
        .expect("an arm")
        .pattern()
        .expect("a pattern")
}

#[test]
fn pattern_bind_wildcard_literal() {
    let Pattern::Bind(b) = arm_pattern("count") else {
        panic!("expected bind pattern");
    };
    assert_eq!(b.name(), Some("count"));

    assert!(matches!(arm_pattern("_"), Pattern::Wildcard(_)));

    let Pattern::Literal(lit) = arm_pattern("42") else {
        panic!("expected literal pattern");
    };
    assert_eq!(
        lit.literal().map(|l| l.text().to_owned()).as_deref(),
        Some("42")
    );
}

#[test]
fn pattern_tuple() {
    let Pattern::Tuple(t) = arm_pattern("(x, _, 1)") else {
        panic!("expected tuple pattern");
    };
    let kinds: Vec<&str> = t
        .elements()
        .map(|p| match p {
            Pattern::Bind(_) => "bind",
            Pattern::Wildcard(_) => "wild",
            Pattern::Literal(_) => "lit",
            Pattern::Constructor(_) => "ctor",
            Pattern::Tuple(_) => "tuple",
        })
        .collect();
    assert_eq!(kinds, ["bind", "wild", "lit"]);
}

#[test]
fn pattern_constructor_nested() {
    // `Some(Cons(x, _))`: a constructor whose field is itself a constructor
    // pattern binding `x` and discarding the tail.
    let Pattern::Constructor(some) = arm_pattern("Some(Cons(x, _))") else {
        panic!("expected constructor pattern");
    };
    assert_eq!(some.name(), Some("Some"));

    let outer: Vec<_> = some.fields().collect();
    assert_eq!(outer.len(), 1);
    let Pattern::Constructor(cons) = &outer[0] else {
        panic!("expected nested constructor");
    };
    assert_eq!(cons.name(), Some("Cons"));

    let sub: Vec<_> = cons.fields().collect();
    assert!(matches!(sub[0], Pattern::Bind(_)));
    assert!(matches!(sub[1], Pattern::Wildcard(_)));
}

#[test]
fn match_arm_patterns() {
    let Expr::Match(m) = body("match msg { PlanRepo(path) -> 1, Shutdown -> 2, _ -> 3 }") else {
        panic!("expected match");
    };
    let names: Vec<String> = m
        .arms()
        .map(|a| match a.pattern() {
            Some(Pattern::Constructor(c)) => c.name().unwrap_or("?").to_owned(),
            Some(Pattern::Wildcard(_)) => "_".to_owned(),
            _ => "other".to_owned(),
        })
        .collect();
    assert_eq!(names, ["PlanRepo", "Shutdown", "_"]);
}

// ── actor body projection ───────────────────────────────────────

#[test]
fn actor_body_members() {
    let src = "\
actor Planner {
  state: St,
  message: Msg = | Plan(Path) | Stop,
  init: fn(s: St) -> St ! {Log} = boot(s),
  handle Plan(p), st -> St ! {Log} = go(p, st),
  handle Stop, st -> St = st,
} ! {Log}";
    let actor_file = file(src);
    let actor = actor_file
        .declarations()
        .find_map(|d| match d {
            Decl::Actor(a) => Some(a),
            _ => None,
        })
        .unwrap();

    let names: Vec<String> = actor
        .fields()
        .filter_map(|f| f.name().map(str::to_owned))
        .collect();
    assert_eq!(names, ["state", "message", "init"]);
    assert!(actor.effect_ann().is_some());

    let fields: Vec<_> = actor.fields().collect();
    // `state` is a plain type.
    assert!(matches!(fields[0].ty(), Some(TypeExpr::Name(n)) if n.text() == "St"));
    assert!(fields[0].fn_sig().is_none());
    // `message` names its sum type and lists constructors.
    assert!(matches!(fields[1].ty(), Some(TypeExpr::Name(n)) if n.text() == "Msg"));
    let ctors: Vec<String> = fields[1]
        .constructors()
        .filter_map(|c| c.name().map(str::to_owned))
        .collect();
    assert_eq!(ctors, ["Plan", "Stop"]);
    assert!(fields[1].body().is_none(), "constructor tail is not a body");
    // `init` is a signature plus body; its `ty()` is None.
    let init = &fields[2];
    assert!(init.ty().is_none());
    let sig = init.fn_sig().expect("init has a signature");
    assert_eq!(sig.params().count(), 1);
    assert!(sig.return_type().is_some());
    assert!(sig.effect_ann().is_some());
    assert!(matches!(init.body(), Some(Expr::App(_))));

    // Handlers: message pattern, state pattern, return type, row, body.
    let handlers: Vec<_> = actor.handlers().collect();
    assert_eq!(handlers.len(), 2);
    let plan = &handlers[0];
    assert!(matches!(
        plan.message_pattern(),
        Some(Pattern::Constructor(ref c)) if c.name() == Some("Plan")
    ));
    assert!(matches!(
        plan.state_pattern(),
        Some(Pattern::Bind(ref b)) if b.name() == Some("st")
    ));
    assert!(plan.return_type().is_some());
    assert!(plan.effect_ann().is_some());
    assert!(matches!(plan.body(), Some(Expr::App(_))));
    assert!(handlers[1].effect_ann().is_none());
}

#[test]
fn spawn_expr_projection() {
    let Expr::Spawn(spawn) = body("spawn(Planner, config, 2)") else {
        panic!("expected a spawn expression");
    };
    assert_eq!(spawn.actor_name(), Some("Planner"));
    let args: Vec<Expr> = spawn.args().collect();
    assert_eq!(args.len(), 2);
    assert!(matches!(&args[0], Expr::Name(n) if n.text() == "config"));
    assert!(matches!(&args[1], Expr::Literal(_)));
}

#[test]
fn spawn_expr_no_args() {
    let Expr::Spawn(spawn) = body("spawn(Worker)") else {
        panic!("expected a spawn expression");
    };
    assert_eq!(spawn.actor_name(), Some("Worker"));
    assert_eq!(spawn.args().count(), 0);
}

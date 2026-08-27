// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Erlang source emission: one IR module to a set of `.erl` files.
//!
//! [`emit_modules`] renders the base module — functions and extern stubs —
//! plus one `gen_server` behaviour module per actor declaration and one
//! `supervisor` behaviour module per supervisor declaration. The output
//! compiles with stock `erlc` and is formatted for human reading. Types are
//! erased (no forms for type or tool declarations); each form is headed by a
//! `%% <file>:<line>` comment from its declaration span.
//!
//! # Calling convention
//!
//! The convention is decided by the function *type*: a function whose effect
//! row is non-empty or open takes one extra trailing parameter, a map from
//! effect keys to handler implementations; a pure function keeps its surface
//! arity. The rule is uniform across named functions and lambdas, and a pure
//! function value meeting an effectful function type (or vice versa) is
//! eta-expanded at the use site to absorb or supply the map.
//!
//! - A `handle` block emits map extension over the in-scope map (or `#{}`
//!   in a pure context); each arm normalises to a binary
//!   `fun(Args, Handlers)` entry so the dispatcher can invoke any entry
//!   uniformly. Map keys are `{tool, name}` for `Tool<Name>` effects and the
//!   snake-cased head atom for bare effects.
//! - A tool call site always emits
//!   `hird_tool_dispatch:call(tool_name, Caller, Handlers, Args)` — never a
//!   direct handler invocation — so audit capture is unconditional. `Caller`
//!   is a binary literal naming the enclosing form (`"Module.function"`,
//!   `"Actor.init"`, or `"Actor.handle_msg/Ctor"`), statically known at
//!   every dispatch site.
//! - A module declaring tools also emits a `hird_tools@/0` signature table —
//!   wire names and value shapes for every tool and declared ADT — which the
//!   audit sink's type-directed record encoder consumes. The `@` keeps the
//!   name outside the image of the Hirð renaming.
//!
//! # Value mapping
//!
//! Constructors become tagged tuples (`Cons(1, Nil)` → `{cons, 1, nil}`),
//! nullary constructors bare atoms (so `Bool` lands on Erlang's `true` /
//! `false`), records become maps, strings UTF-8 binaries, and unit the atom
//! `ok`. `match` becomes `case`; `let` becomes a match expression, wrapped in
//! `begin … end` when it sits in expression position. The messaging
//! primitives lower per the actor mapping: `spawn` to the actor module's
//! `start_link`, `send` to `gen_server:cast`, `request` to `gen_server:call`
//! with the fixed 5000 ms timeout, `reply` to `gen_server:reply`, and
//! `crash!` to `erlang:error`.
//!
//! # Actor modules
//!
//! An actor emits a `gen_server` behaviour module exporting `start_link` (at
//! the init arity) and the three required callbacks. Dispatch is per message
//! constructor: a constructor whose declaration carries a `ReplyTo` field is
//! received by a `handle_call` clause — the payload is the bare constructor
//! atom, the reply channel binds `From` — and every other constructor by a
//! `handle_cast` clause matching its ADT wire shape. Every clause returns
//! `{noreply, NextState}`; replies are always explicit `gen_server:reply`
//! calls in handler bodies. Handler maps never cross the spawn boundary:
//! callbacks run init and handler bodies against no in-scope map, so tool
//! calls inside actors fall back to the runtime registry.
//!
//! # Supervisor modules
//!
//! A supervisor emits a `supervisor` behaviour module exporting
//! `start_link/0` — registering the process as `{local, Module}` — and
//! `init/1`, which builds the flags map (strategy rendered verbatim,
//! intensity, period) and one child-spec map per child: id, a start MFA
//! through the actor module's `start_link/1`, the restart disposition, and an
//! explicit `worker` type (`shutdown` is left to the OTP default). Children
//! stay unregistered; `start_args` is pure, so it renders against no in-scope
//! handler map.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_ir::{
    IrActorDef, IrActorHandler, IrApp, IrChildSpec, IrConstructor, IrConstructorPat, IrDecl,
    IrExpr, IrExternRef, IrFnDef, IrHandle, IrInstall, IrLambda, IrLet, IrMatch, IrModule,
    IrPattern, IrSpan, IrSupervisorDef, IrToolDef, IrTypeDef, IrVar, LiteralValue,
};
use hird_types::{Effect, EffectRow, Type};

use crate::names::{atom, erlang_module_name, snake_case, variable_base};

/// One emitted Erlang source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedModule {
    /// The Erlang module name (also the target `.erl` file stem).
    pub name: String,
    /// The Erlang source text.
    pub source: String,
}

/// Renders `module` as Erlang source files: the base module (functions and
/// extern stubs) first, then one behaviour module per actor (`gen_server`)
/// and supervisor (`supervisor`) declaration, in source order.
///
/// `source_path` is the Hirð source the module was lowered from; it appears
/// in each file's generated-file banner and `%% <file>:<line>` comments.
/// Each output should be written to `<its name>.erl`.
#[must_use]
pub fn emit_modules(module: &IrModule, source_path: &str) -> Vec<EmittedModule> {
    let mut emitter = Emitter::new(module);
    let base = erlang_module_name(&module.name);
    let mut out = Vec::from([EmittedModule {
        name: base.clone(),
        source: emitter.module(source_path),
    }]);
    emitter.remote = Some(base);
    for decl in &module.declarations {
        match decl {
            IrDecl::Actor(actor) => out.push(EmittedModule {
                name: erlang_module_name(&actor.name),
                source: emitter.actor_module(actor, source_path),
            }),
            IrDecl::Supervisor(sup) => out.push(EmittedModule {
                name: erlang_module_name(&sup.name),
                source: emitter.supervisor_module(sup, source_path),
            }),
            _ => {}
        }
    }
    out
}

/// The indentation unit (four spaces).
const INDENT: &str = "    ";

/// `level` indentation units.
fn ind(level: usize) -> String {
    INDENT.repeat(level)
}

/// Whether a function of this row takes the trailing handler-map parameter:
/// yes when the row is non-empty or open (an open row may instantiate to
/// anything, so the map must be threadable).
fn takes_map(row: &EffectRow) -> bool {
    !row.is_empty()
}

/// Emission position: whether an expression may unfold into a comma-separated
/// match sequence (a function, fun, or case-arm body) or must stay a single
/// expression (an argument, operand, or bound value).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    /// A body position: `X = V, …` sequences are legal.
    Body,
    /// A single-expression position: sequences wrap in `begin … end`.
    Expr,
}

/// A local binding: its Erlang variable and its binding-site type.
///
/// The type is the one recorded where the name was bound (parameter
/// annotation, bound value, pattern), which preserves open effect rows —
/// unlike a use site's instantiation, which may have collapsed them — so
/// calling conventions read off it stay coherent with the definition.
#[derive(Clone)]
struct Binding {
    /// The Erlang variable name.
    var: String,
    /// The binding-site type.
    ty: Type,
}

/// The lexical environment: local bindings, the in-scope handler map, and
/// the enclosing form's caller id.
#[derive(Clone, Default)]
struct Env {
    /// Hirð name → binding, innermost shadowing outermost.
    scope: BTreeMap<String, Binding>,
    /// The in-scope handler-map variable; `None` in a pure context, where a
    /// needed map falls back to `#{}`.
    handlers: Option<String>,
    /// The caller id injected at tool dispatch sites: `Module.function` for
    /// module functions, `Actor.init` / `Actor.handle_msg/Ctor` inside actor
    /// callbacks. Lambdas inherit the enclosing form's id.
    caller: String,
}

/// Per-function mutable state: variable freshness and usage.
#[derive(Default)]
struct FnCx {
    /// Every Erlang variable name allocated so far.
    used: BTreeSet<String>,
    /// Every variable actually referenced by emitted code, so unused
    /// parameters can be `_`-prefixed in the head.
    referenced: BTreeSet<String>,
    /// Counter for `@N`-suffixed fresh names.
    counter: u32,
}

impl FnCx {
    /// A fresh Erlang variable for the Hirð binder `name`, `@N`-suffixed on
    /// collision with an already-allocated name. `_` stays anonymous.
    fn fresh_var(&mut self, name: &str) -> String {
        let base = variable_base(name);
        if base == "_" {
            return base;
        }
        if self.used.insert(base.clone()) {
            return base;
        }
        loop {
            self.counter += 1;
            let candidate = format!("{base}@{}", self.counter);
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    /// A fresh emitter-internal variable `Prefix@N`. The `@` keeps it outside
    /// the image of the Hirð renaming, so it can never collide with source
    /// binders.
    fn fresh_internal(&mut self, prefix: &str) -> String {
        loop {
            self.counter += 1;
            let candidate = format!("{prefix}@{}", self.counter);
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    /// `var` as it should print in a binder head: `_`-prefixed when the body
    /// never referenced it (suppressing the unused-variable warning), kept
    /// verbatim when prefixing would collide with an existing name.
    fn head_var(&self, var: &str) -> String {
        if var == "_" || self.referenced.contains(var) {
            return String::from(var);
        }
        let prefixed = format!("_{var}");
        if self.used.contains(&prefixed) {
            String::from(var)
        } else {
            prefixed
        }
    }
}

/// The per-module emitter: tool and function tables shared by every form.
struct Emitter<'a> {
    /// The module being emitted.
    module: &'a IrModule,
    /// Tool function name (`read_repo`) → the tool function's type, with the
    /// implicit `Tool<Marker>` effect included in its row.
    tools: BTreeMap<String, Type>,
    /// The tool declarations, in source order (the signature table's rows).
    tool_defs: Vec<&'a IrToolDef>,
    /// Declared ADT name → its definition (module types and actor message
    /// types), for the signature table's constructor shapes.
    type_defs: BTreeMap<&'a str, &'a IrTypeDef>,
    /// Module-level function or extern name → its function type (declared
    /// parameter and row, not a use site's instantiation).
    fns: BTreeMap<String, Type>,
    /// The Erlang module to qualify module-level function references with;
    /// `None` while emitting the base module itself (calls stay local).
    remote: Option<String>,
}

impl<'a> Emitter<'a> {
    /// Builds the tool and function tables for `module`.
    fn new(module: &'a IrModule) -> Self {
        let mut tools = BTreeMap::new();
        let mut tool_defs = Vec::new();
        let mut type_defs = BTreeMap::new();
        let mut fns = BTreeMap::new();
        for decl in &module.declarations {
            match decl {
                IrDecl::Fn(f) => {
                    fns.insert(f.name.clone(), fn_def_type(f));
                }
                IrDecl::Extern(e) => {
                    fns.insert(e.name.clone(), unquantified(&e.ty).clone());
                }
                IrDecl::Tool(t) => {
                    let mut row = t.effect_row.clone();
                    row.insert(Effect::parametric(
                        "Tool",
                        Vec::from([Type::con(t.name.as_str(), Vec::new())]),
                    ));
                    let ty = Type::TyFn(
                        Vec::from([t.input.clone()]),
                        Box::new(t.output.clone()),
                        row,
                    );
                    tools.insert(snake_case(&t.name), ty);
                    tool_defs.push(t);
                }
                IrDecl::Type(t) => {
                    type_defs.insert(t.name.as_str(), t);
                }
                IrDecl::Actor(a) => {
                    type_defs.insert(a.message.name.as_str(), &a.message);
                }
                IrDecl::Supervisor(_) => {}
            }
        }
        Self {
            module,
            tools,
            tool_defs,
            type_defs,
            fns,
            remote: None,
        }
    }

    // ── module ───────────────────────────────────────────────────

    /// The whole `.erl` file: banner, module and export attributes, then one
    /// form per function or extern declaration.
    fn module(&self, source_path: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "%% Generated from {source_path} by the Hirð compiler. Do not edit.\n"
        ));
        out.push_str(&format!(
            "-module({}).\n",
            erlang_module_name(&self.module.name)
        ));
        let exports = self.exports();
        if !exports.is_empty() {
            out.push_str(&format!("-export([{}]).\n", exports.join(", ")));
        }
        for decl in &self.module.declarations {
            match decl {
                IrDecl::Fn(f) => {
                    out.push('\n');
                    self.fn_form(f, source_path, &mut out);
                }
                IrDecl::Extern(e) => {
                    out.push('\n');
                    self.extern_form(e, source_path, &mut out);
                }
                // Types are erased; tools exist only as dispatcher call
                // sites and signature-table rows; actor and supervisor
                // behaviour modules are emitted separately.
                IrDecl::Type(_) | IrDecl::Tool(_) | IrDecl::Actor(_) | IrDecl::Supervisor(_) => {}
            }
        }
        if !self.tool_defs.is_empty() {
            out.push('\n');
            self.tool_table_form(&mut out);
        }
        out
    }

    /// The export list: every function and extern at its emitted arity
    /// (surface arity plus one for the handler map when the row asks for it),
    /// plus the signature table when the module declares tools.
    fn exports(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .module
            .declarations
            .iter()
            .filter_map(|decl| {
                let (name, ty) = match decl {
                    IrDecl::Fn(f) => (&f.name, self.fns.get(&f.name)?),
                    IrDecl::Extern(e) => (&e.name, self.fns.get(&e.name)?),
                    _ => return None,
                };
                Some(format!("{}/{}", atom(name), emitted_arity(ty)))
            })
            .collect();
        if !self.tool_defs.is_empty() {
            out.push(String::from("hird_tools@/0"));
        }
        out
    }

    /// The `%% <file>:<line>` comment above a form (omitted for an unknown
    /// span).
    fn span_comment(span: IrSpan, source_path: &str, out: &mut String) {
        if span.line > 0 {
            out.push_str(&format!("%% {source_path}:{}\n", span.line));
        }
    }

    /// One function form: span comment, head with renamed parameters (plus
    /// the trailing handler map when the declared row asks for it), body.
    fn fn_form(&self, f: &IrFnDef, source_path: &str, out: &mut String) {
        let mut cx = FnCx::default();
        let mut env = Env {
            caller: format!("{}.{}", self.module.name, f.name),
            ..Env::default()
        };
        let mut params: Vec<String> = Vec::new();
        for param in &f.params {
            let var = cx.fresh_var(&param.name);
            env.scope.insert(
                param.name.clone(),
                Binding {
                    var: var.clone(),
                    ty: param.ty.clone(),
                },
            );
            params.push(var);
        }
        if takes_map(&f.effect_row) {
            let var = String::from("Handlers@");
            cx.used.insert(var.clone());
            env.handlers = Some(var.clone());
            params.push(var);
        }
        let body = self.expr(&f.body, &env, &mut cx, 1, Ctx::Body);
        let heads: Vec<String> = params.iter().map(|p| cx.head_var(p)).collect();
        Self::span_comment(f.span, source_path, out);
        out.push_str(&format!(
            "{}({}) ->\n{}{body}.\n",
            atom(&f.name),
            heads.join(", "),
            ind(1)
        ));
    }

    /// An extern stub: the declared arity, crashing until an FFI module can
    /// be named (`v0.1` externs have no backing module).
    fn extern_form(&self, e: &IrExternRef, source_path: &str, out: &mut String) {
        let arity = self.fns.get(&e.name).map_or(0, emitted_arity);
        let params: Vec<&str> = (0..arity).map(|_| "_").collect();
        Self::span_comment(e.span, source_path, out);
        out.push_str(&format!(
            "{}({}) ->\n{}erlang:error({{unbound_extern, {}}}).\n",
            atom(&e.name),
            params.join(", "),
            ind(1),
            atom(&e.name)
        ));
    }

    // ── tool signature table ─────────────────────────────────────

    /// The `hird_tools@/0` form: per-tool wire names and value shapes plus
    /// the declared ADTs' constructor shapes, consumed by the audit sink's
    /// type-directed record encoder.
    fn tool_table_form(&self, out: &mut String) {
        let tools: Vec<String> = self
            .tool_defs
            .iter()
            .map(|t| {
                format!(
                    "{} => #{{\n{i}name => <<\"{}\"/utf8>>,\n{i}args => {},\n{i}result => {},\n{i}error => {}}}",
                    atom(&snake_case(&t.name)),
                    t.name,
                    self.wire_shape(&t.input, &t.params, false),
                    self.wire_shape(&t.output, &t.params, false),
                    self.error_shape(t),
                    i = ind(4),
                )
            })
            .collect();
        let types: Vec<String> = self
            .type_defs
            .values()
            .map(|def| {
                let ctors: Vec<String> = def
                    .constructors
                    .iter()
                    .map(|ctor| {
                        let fields: Vec<String> = ctor
                            .fields
                            .iter()
                            .map(|f| self.wire_shape(f, &def.params, true))
                            .collect();
                        format!(
                            "{{{}, <<\"{}\"/utf8>>, [{}]}}",
                            atom(&snake_case(&ctor.name)),
                            ctor.name,
                            fields.join(", ")
                        )
                    })
                    .collect();
                format!(
                    "{} => [{}]",
                    atom(&snake_case(&def.name)),
                    ctors.join(&format!(",\n{}", ind(4)))
                )
            })
            .collect();
        let types_map = if types.is_empty() {
            String::from("#{}")
        } else {
            format!("#{{\n{}{}}}", ind(3), types.join(&format!(",\n{}", ind(3))))
        };
        out.push_str(&format!(
            "hird_tools@() ->\n\
             {i}#{{tools => #{{\n\
             {iii}{}}},\n\
             {ii}types => {types_map}}}.\n",
            tools.join(&format!(",\n{}", ind(3))),
            i = ind(1),
            ii = ind(2),
            iii = ind(3),
        ));
    }

    /// A type's wire shape in the signature table: `unit`, `int`, `float`,
    /// `string`, `bool`, `{list, S}`, `{tuple, [S…]}`,
    /// `{record, [{label, S}…]}` (sorted labels), or `{adt, name, [S…]}` for
    /// a declared ADT. A declaration type parameter renders as `{param, N}`
    /// inside an ADT's constructor shapes (`as_param`) and as `dynamic` in a
    /// generic tool's signature, whose instantiation is a call-site fact the
    /// table cannot carry; anything else non-representable is `dynamic` too.
    fn wire_shape(&self, ty: &Type, params: &[String], as_param: bool) -> String {
        match unquantified(ty) {
            Type::TyTuple(elems) if elems.is_empty() => String::from("unit"),
            Type::TyTuple(elems) => {
                let shapes: Vec<String> = elems
                    .iter()
                    .map(|e| self.wire_shape(e, params, as_param))
                    .collect();
                format!("{{tuple, [{}]}}", shapes.join(", "))
            }
            Type::TyRecord(fields) => {
                let shapes: Vec<String> = fields
                    .iter()
                    .map(|(label, field)| {
                        format!(
                            "{{{}, {}}}",
                            atom(label.as_str()),
                            self.wire_shape(field, params, as_param)
                        )
                    })
                    .collect();
                format!("{{record, [{}]}}", shapes.join(", "))
            }
            Type::TyCon(name, args) => match (name.as_str(), args.as_slice()) {
                ("Int", []) => String::from("int"),
                ("Float", []) => String::from("float"),
                ("String", []) => String::from("string"),
                ("Bool", []) => String::from("bool"),
                ("List", [elem]) => {
                    format!("{{list, {}}}", self.wire_shape(elem, params, as_param))
                }
                (n, []) if params.iter().any(|p| p == n) => {
                    if as_param {
                        let index = params.iter().position(|p| p == n).unwrap_or(0);
                        format!("{{param, {index}}}")
                    } else {
                        String::from("dynamic")
                    }
                }
                (n, args) if self.type_defs.contains_key(n) => {
                    let shapes: Vec<String> = args
                        .iter()
                        .map(|a| self.wire_shape(a, params, as_param))
                        .collect();
                    format!("{{adt, {}, [{}]}}", atom(&snake_case(n)), shapes.join(", "))
                }
                _ => String::from("dynamic"),
            },
            Type::TyVar(_) | Type::TyFn(..) | Type::TyForall(..) => String::from("dynamic"),
        }
    }

    /// The shape of a tool's `err` results: its trailing row's single
    /// `Exn<E>` error type, or `dynamic` when the row carries none (no `err`
    /// is producible) or several (the value alone cannot pick one).
    fn error_shape(&self, tool: &IrToolDef) -> String {
        let mut exns = tool
            .effect_row
            .effects()
            .filter(|e| e.head().as_str() == "Exn");
        match (exns.next(), exns.next()) {
            (Some(e), None) if e.args().len() == 1 => {
                self.wire_shape(&e.args()[0], &tool.params, false)
            }
            _ => String::from("dynamic"),
        }
    }

    // ── actor modules ────────────────────────────────────────────

    /// One actor as a `gen_server` behaviour module: banner, span comment,
    /// module/behaviour/export attributes, `start_link` at the init arity,
    /// and the three required callbacks. Callback bodies run against no
    /// in-scope handler map, so their tool calls fall back to the runtime
    /// registry.
    fn actor_module(&self, actor: &IrActorDef, source_path: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "%% Generated from {source_path} by the Hirð compiler. Do not edit.\n"
        ));
        Self::span_comment(actor.span, source_path, &mut out);
        out.push_str(&format!("-module({}).\n", erlang_module_name(&actor.name)));
        out.push_str("-behaviour(gen_server).\n");
        out.push_str(&format!(
            "-export([start_link/{}]).\n",
            actor.init.params.len()
        ));
        out.push_str("-export([init/1, handle_call/3, handle_cast/2]).\n");
        self.start_link_form(actor, &mut out);
        self.init_form(actor, &mut out);
        let (calls, casts) = split_handlers(actor);
        self.handle_call_form(&actor.name, &calls, &mut out);
        self.handle_cast_form(&actor.name, &casts, &mut out);
        out
    }

    /// The `start_link` form: the init parameters at surface arity, passed to
    /// `gen_server:start_link` packed as the single `init/1` argument.
    fn start_link_form(&self, actor: &IrActorDef, out: &mut String) {
        let mut cx = FnCx::default();
        let params: Vec<String> = actor
            .init
            .params
            .iter()
            .map(|p| cx.fresh_var(&p.name))
            .collect();
        out.push_str(&format!(
            "\nstart_link({}) ->\n{}gen_server:start_link(?MODULE, {}, []).\n",
            params.join(", "),
            ind(1),
            init_arg(&params)
        ));
    }

    /// The `init/1` callback: unpack the init parameters, run the Hirð init
    /// body, and wrap the initial state in `{ok, _}`.
    fn init_form(&self, actor: &IrActorDef, out: &mut String) {
        let mut cx = FnCx::default();
        let mut env = Env {
            caller: format!("{}.init", actor.name),
            ..Env::default()
        };
        let mut params: Vec<String> = Vec::new();
        for param in &actor.init.params {
            let var = cx.fresh_var(&param.name);
            env.scope.insert(
                param.name.clone(),
                Binding {
                    var: var.clone(),
                    ty: param.ty.clone(),
                },
            );
            params.push(var);
        }
        let body = self.expr(&actor.init.body, &env, &mut cx, 1, Ctx::Expr);
        let heads: Vec<String> = params.iter().map(|p| cx.head_var(p)).collect();
        out.push_str(&format!(
            "\ninit({}) ->\n{}{{ok, {body}}}.\n",
            init_arg(&heads),
            ind(1)
        ));
    }

    /// The `handle_call/3` callback: one clause per call constructor — the
    /// bare constructor atom as payload, the reply channel bound from `From`,
    /// an explicit `{noreply, NextState}` — or a crashing fallback clause
    /// when the actor has no call constructors.
    fn handle_call_form(&self, actor: &str, clauses: &[CallClause<'_>], out: &mut String) {
        if clauses.is_empty() {
            out.push_str(&format!(
                "\nhandle_call(Request, _From, _State) ->\n{}erlang:error({{unexpected_call, Request}}).\n",
                ind(1)
            ));
            return;
        }
        out.push('\n');
        for (i, clause) in clauses.iter().enumerate() {
            let mut cx = FnCx::default();
            let mut env = Env {
                caller: format!("{actor}.handle_msg/{}", clause.ctor.name),
                ..Env::default()
            };
            let tag = atom(&snake_case(&clause.ctor.name));
            let from = clause
                .ctor
                .fields
                .get(clause.reply_pos)
                .map_or_else(|| String::from("_"), |p| self.pattern(p, &mut env, &mut cx));
            let state = self.pattern(&clause.handler.state, &mut env, &mut cx);
            let body = self.expr(&clause.handler.body, &env, &mut cx, 1, Ctx::Expr);
            let sep = if i + 1 == clauses.len() { "." } else { ";" };
            out.push_str(&format!(
                "handle_call({tag}, {from}, {state}) ->\n{}{{noreply, {body}}}{sep}\n",
                ind(1)
            ));
        }
    }

    /// The `handle_cast/2` callback: one clause per cast constructor,
    /// matching its ADT wire shape and returning `{noreply, NextState}`, or
    /// a crashing fallback clause when the actor has no cast constructors.
    fn handle_cast_form(&self, actor: &str, handlers: &[&IrActorHandler], out: &mut String) {
        if handlers.is_empty() {
            out.push_str(&format!(
                "\nhandle_cast(Message, _State) ->\n{}erlang:error({{unexpected_cast, Message}}).\n",
                ind(1)
            ));
            return;
        }
        out.push('\n');
        for (i, handler) in handlers.iter().enumerate() {
            let mut cx = FnCx::default();
            let mut env = Env {
                caller: match &handler.message {
                    IrPattern::Constructor(ctor) => {
                        format!("{actor}.handle_msg/{}", ctor.name)
                    }
                    _ => format!("{actor}.handle_msg"),
                },
                ..Env::default()
            };
            let message = self.pattern(&handler.message, &mut env, &mut cx);
            let state = self.pattern(&handler.state, &mut env, &mut cx);
            let body = self.expr(&handler.body, &env, &mut cx, 1, Ctx::Expr);
            let sep = if i + 1 == handlers.len() { "." } else { ";" };
            out.push_str(&format!(
                "handle_cast({message}, {state}) ->\n{}{{noreply, {body}}}{sep}\n",
                ind(1)
            ));
        }
    }

    // ── supervisor modules ───────────────────────────────────────

    /// One supervisor as a `supervisor` behaviour module: banner, span
    /// comment, module/behaviour/export attributes, `start_link/0`
    /// registering the process as `{local, Module}`, and `init/1`.
    fn supervisor_module(&self, sup: &IrSupervisorDef, source_path: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "%% Generated from {source_path} by the Hirð compiler. Do not edit.\n"
        ));
        Self::span_comment(sup.span, source_path, &mut out);
        out.push_str(&format!("-module({}).\n", erlang_module_name(&sup.name)));
        out.push_str("-behaviour(supervisor).\n");
        out.push_str("-export([start_link/0]).\n");
        out.push_str("-export([init/1]).\n");
        out.push_str(&format!(
            "\nstart_link() ->\n{}supervisor:start_link({{local, ?MODULE}}, ?MODULE, []).\n",
            ind(1)
        ));
        self.sup_init_form(sup, &mut out);
        out
    }

    /// The supervisor `init/1` callback: the flags map (strategy rendered
    /// verbatim, intensity, period) and the child-spec list. One variable
    /// context spans every child, so `start_args` bindings stay distinct
    /// within the shared function scope.
    fn sup_init_form(&self, sup: &IrSupervisorDef, out: &mut String) {
        let mut cx = FnCx::default();
        cx.used.insert(String::from("SupFlags"));
        cx.used.insert(String::from("ChildSpecs"));
        let children: Vec<String> = sup
            .children
            .iter()
            .map(|child| self.child_spec(&sup.name, child, &mut cx))
            .collect();
        let specs = if children.is_empty() {
            String::from("[]")
        } else {
            format!(
                "[\n{}{}\n{}]",
                ind(2),
                children.join(&format!(",\n{}", ind(2))),
                ind(1)
            )
        };
        out.push_str(&format!(
            "\ninit([]) ->\n\
             {i}SupFlags = #{{\n\
             {ii}strategy => {},\n\
             {ii}intensity => {},\n\
             {ii}period => {}\n\
             {i}}},\n\
             {i}ChildSpecs = {specs},\n\
             {i}{{ok, {{SupFlags, ChildSpecs}}}}.\n",
            atom(&sup.strategy),
            sup.intensity,
            sup.period,
            i = ind(1),
            ii = ind(2),
        ));
    }

    /// One child-spec map: id, a start MFA through the actor module's
    /// `start_link/1` (`start_args` is pure, so it renders against no
    /// in-scope handler map), the restart disposition, and an explicit
    /// `worker` type (`shutdown` is left to the OTP default). Children stay
    /// unregistered.
    fn child_spec(&self, sup: &str, child: &IrChildSpec, cx: &mut FnCx) -> String {
        let env = Env {
            caller: format!("{sup}.init"),
            ..Env::default()
        };
        let arg = self.expr(&child.start_args, &env, cx, 3, Ctx::Expr);
        format!(
            "#{{\n\
             {i}id => {},\n\
             {i}start => {{{}, start_link, [{arg}]}},\n\
             {i}restart => {},\n\
             {i}type => worker\n\
             {}}}",
            atom(&child.id),
            erlang_module_name(&child.actor),
            atom(&child.restart),
            ind(2),
            i = ind(3),
        )
    }

    // ── expressions ──────────────────────────────────────────────

    /// Renders one expression. The caller places the first line; embedded
    /// lines indent relative to `indent`.
    fn expr(&self, expr: &IrExpr, env: &Env, cx: &mut FnCx, indent: usize, ctx: Ctx) -> String {
        match expr {
            IrExpr::Literal(lit) => literal(&lit.value),
            IrExpr::Var(v) => self.var_value(v, env, cx),
            IrExpr::Let(le) => self.let_expr(le, env, cx, indent, ctx),
            IrExpr::Lambda(lambda) => self.lambda(lambda, env, cx, indent),
            IrExpr::App(app) => self.app(app, env, cx, indent),
            IrExpr::Match(m) => self.match_expr(m, env, cx, indent),
            IrExpr::Handle(h) => self.handle(h, env, cx, indent, ctx),
            IrExpr::Install(inst) => self.install(inst, env, cx, indent),
            IrExpr::Spawn(spawn) => {
                let args: Vec<String> = spawn
                    .args
                    .iter()
                    .map(|a| self.expr(a, env, cx, indent, Ctx::Expr))
                    .collect();
                let pid = cx.fresh_internal("Pid");
                let start = format!(
                    "{{ok, {pid}}} = {}:start_link({}),",
                    erlang_module_name(&spawn.actor),
                    args.join(", ")
                );
                sequence(&[start, pid], indent, ctx)
            }
            IrExpr::Supervise(supervise) => {
                let start = format!(
                    "{{ok, _}} = {}:start_link(),",
                    erlang_module_name(&supervise.supervisor)
                );
                sequence(&[start, String::from("ok")], indent, ctx)
            }
            IrExpr::Stand(_) => sequence(
                &[String::from("ok = hird_stand:await(),"), String::from("ok")],
                indent,
                ctx,
            ),
            IrExpr::Child(child) => {
                // The runtime lookup yields `{ok, Pid} | error`; the miss is a
                // crash (a missing or restarting child is supervision's
                // concern), rendered inline as a case.
                let pid = cx.fresh_internal("Pid");
                let id = atom(&child.child_id);
                format!(
                    "case hird_sup_util:child_pid({}, {id}) of\
                     \n{}{{ok, {pid}}} -> {pid};\
                     \n{}error -> erlang:error({{no_child, {id}}})\
                     \n{}end",
                    erlang_module_name(&child.supervisor),
                    ind(indent + 1),
                    ind(indent + 1),
                    ind(indent)
                )
            }
            IrExpr::Send(send) => {
                let pid = self.expr(&send.pid, env, cx, indent, Ctx::Expr);
                let msg = self.expr(&send.message, env, cx, indent, Ctx::Expr);
                format!("gen_server:cast({pid}, {msg})")
            }
            IrExpr::Request(request) => {
                let pid = self.expr(&request.pid, env, cx, indent, Ctx::Expr);
                // The checker guarantees the builder is a bare message
                // constructor whose only field is the reply channel, so the
                // wire payload is the bare constructor atom (the `From` term
                // carries the reply address).
                let msg = match request.message_fn.as_ref() {
                    IrExpr::Constructor(ctor) => atom(&snake_case(&ctor.name)),
                    other => self.expr(other, env, cx, indent, Ctx::Expr),
                };
                format!("gen_server:call({pid}, {msg}, 5000)")
            }
            IrExpr::Reply(reply) => {
                let to = self.expr(&reply.reply_to, env, cx, indent, Ctx::Expr);
                let value = self.expr(&reply.value, env, cx, indent, Ctx::Expr);
                format!("gen_server:reply({to}, {value})")
            }
            IrExpr::Crash(crash) => {
                let msg = self.expr(&crash.message, env, cx, indent, Ctx::Expr);
                format!("erlang:error({msg})")
            }
            IrExpr::Constructor(ctor) => self.constructor(ctor, env, cx, indent),
            IrExpr::Tuple(tuple) => {
                if tuple.elems.is_empty() {
                    // Unit is the atom `ok`, the conventional Erlang
                    // don't-care value.
                    return String::from("ok");
                }
                let elems: Vec<String> = tuple
                    .elems
                    .iter()
                    .map(|e| self.expr(e, env, cx, indent, Ctx::Expr))
                    .collect();
                format!("{{{}}}", elems.join(", "))
            }
            IrExpr::List(list) => {
                let elems: Vec<String> = list
                    .elems
                    .iter()
                    .map(|e| self.expr(e, env, cx, indent, Ctx::Expr))
                    .collect();
                format!("[{}]", elems.join(", "))
            }
            IrExpr::Record(record) => {
                let fields: Vec<String> = record
                    .fields
                    .iter()
                    .map(|f| {
                        let value = self.expr(&f.value, env, cx, indent, Ctx::Expr);
                        format!("{} => {value}", atom(&f.label))
                    })
                    .collect();
                format!("#{{{}}}", fields.join(", "))
            }
            IrExpr::Field(field) => {
                let receiver = self.expr(&field.receiver, env, cx, indent, Ctx::Expr);
                format!("maps:get({}, {receiver})", atom(&field.field))
            }
        }
    }

    /// `let name = value in body`: a match expression followed by the body,
    /// flattened into the surrounding sequence in body position.
    fn let_expr(&self, le: &IrLet, env: &Env, cx: &mut FnCx, indent: usize, ctx: Ctx) -> String {
        let inner_indent = if ctx == Ctx::Body { indent } else { indent + 1 };
        let value = self.expr(&le.value, env, cx, inner_indent, Ctx::Expr);
        let var = cx.fresh_var(&le.name);
        let mut inner = env.clone();
        inner.scope.insert(
            le.name.clone(),
            Binding {
                var: var.clone(),
                ty: le.ty.clone(),
            },
        );
        let body = self.expr(&le.body, &inner, cx, inner_indent, Ctx::Body);
        sequence(&[format!("{var} = {value},"), body], indent, ctx)
    }

    /// `λparams → body` as `fun(Params[, Handlers]) -> Body end`. An
    /// effectful lambda binds its own trailing map; a pure lambda's body sees
    /// no map at all (the map travels with calls, it is not captured at fun
    /// creation).
    fn lambda(&self, lambda: &IrLambda, env: &Env, cx: &mut FnCx, indent: usize) -> String {
        let mut inner = env.clone();
        let mut params: Vec<String> = Vec::new();
        for param in &lambda.params {
            let var = cx.fresh_var(&param.name);
            inner.scope.insert(
                param.name.clone(),
                Binding {
                    var: var.clone(),
                    ty: param.ty.clone(),
                },
            );
            params.push(var);
        }
        if takes_map(&lambda.effect_row) {
            let var = cx.fresh_internal("Handlers");
            inner.handlers = Some(var.clone());
            params.push(var);
        } else {
            inner.handlers = None;
        }
        let body = self.expr(&lambda.body, &inner, cx, indent + 1, Ctx::Body);
        let heads: Vec<String> = params.iter().map(|p| cx.head_var(p)).collect();
        format!(
            "fun({}) ->\n{}{body}\n{}end",
            heads.join(", "),
            ind(indent + 1),
            ind(indent)
        )
    }

    /// A constructor: a bare atom when nullary, a tagged tuple otherwise. A
    /// constructor referenced as a function value eta-expands into a fun
    /// building the tuple.
    fn constructor(&self, ctor: &IrConstructor, env: &Env, cx: &mut FnCx, indent: usize) -> String {
        let tag = atom(&snake_case(&ctor.name));
        if let Type::TyFn(params, _, _) = &ctor.result_type
            && ctor.args.is_empty()
            && !params.is_empty()
        {
            let vars: Vec<String> = params.iter().map(|_| cx.fresh_internal("V")).collect();
            return format!(
                "fun({}) -> {{{tag}, {}}} end",
                vars.join(", "),
                vars.join(", ")
            );
        }
        if ctor.args.is_empty() {
            return tag;
        }
        let args: Vec<String> = ctor
            .args
            .iter()
            .map(|a| self.expr(a, env, cx, indent, Ctx::Expr))
            .collect();
        format!("{{{tag}, {}}}", args.join(", "))
    }

    /// `match scrutinee { arms }` as `case … of … end`. Each arm binds in its
    /// own child scope; pattern binders are always fresh, so an outer binding
    /// can never turn a pattern variable into an equality match.
    fn match_expr(&self, m: &IrMatch, env: &Env, cx: &mut FnCx, indent: usize) -> String {
        let scrutinee = self.expr(&m.scrutinee, env, cx, indent, Ctx::Expr);
        let mut out = format!("case {scrutinee} of");
        for (i, arm) in m.arms.iter().enumerate() {
            let mut inner = env.clone();
            let pattern = self.pattern(&arm.pattern, &mut inner, cx);
            let body = self.expr(&arm.body, &inner, cx, indent + 2, Ctx::Body);
            let sep = if i + 1 == m.arms.len() { "" } else { ";" };
            out.push_str(&format!(
                "\n{}{pattern} ->\n{}{body}{sep}",
                ind(indent + 1),
                ind(indent + 2)
            ));
        }
        out.push_str(&format!("\n{}end", ind(indent)));
        out
    }

    /// A `handle` block: bind an extended handler map, then emit the body
    /// against it. Arms merge over the in-scope map (or stand alone in a pure
    /// context) and normalise to binary `fun(Args, Handlers)` entries.
    fn handle(&self, h: &IrHandle, env: &Env, cx: &mut FnCx, indent: usize, ctx: Ctx) -> String {
        let inner_indent = if ctx == Ctx::Body { indent } else { indent + 1 };
        let entries: Vec<String> = h
            .arms
            .iter()
            .map(|arm| {
                let key = effect_key(&arm.effect);
                let entry = self.handler_entry(&arm.handler, env, cx, inner_indent + 1);
                format!("{key} => {entry}")
            })
            .collect();
        let map_lit = format!(
            "#{{\n{}{}\n{}}}",
            ind(inner_indent + 1),
            entries.join(&format!(",\n{}", ind(inner_indent + 1))),
            ind(inner_indent)
        );
        let merged = match &env.handlers {
            Some(base) => {
                cx.referenced.insert(base.clone());
                format!("maps:merge({base}, {map_lit})")
            }
            None => map_lit,
        };
        let var = cx.fresh_internal("Handlers");
        let mut inner = env.clone();
        inner.handlers = Some(var.clone());
        let body = self.expr(&h.body, &inner, cx, inner_indent, Ctx::Body);
        sequence(&[format!("{var} = {merged},"), body], indent, ctx)
    }

    /// An `install` block: `hird_handlers:with_handlers(Entries, fun() ->
    /// Body end)`. Entries land in the runtime's process-independent default
    /// registry for the dynamic extent of the body (restored afterwards,
    /// crash included) — where spawned actors' tool calls resolve. Keys and
    /// binary-fun normalisation are the handler map's; the body itself still
    /// runs against the unchanged in-scope map.
    fn install(&self, inst: &IrInstall, env: &Env, cx: &mut FnCx, indent: usize) -> String {
        let entries: Vec<String> = inst
            .arms
            .iter()
            .map(|arm| {
                let key = effect_key(&arm.effect);
                let entry = self.handler_entry(&arm.handler, env, cx, indent + 2);
                format!("{{{key}, {entry}}}")
            })
            .collect();
        let list = format!(
            "[\n{}{}\n{}]",
            ind(indent + 2),
            entries.join(&format!(",\n{}", ind(indent + 2))),
            ind(indent + 1)
        );
        let body = self.expr(&inst.body, env, cx, indent + 2, Ctx::Body);
        format!(
            "hird_handlers:with_handlers(\n{}{list},\n{}fun() ->\n{}{body}\n{}end)",
            ind(indent + 1),
            ind(indent + 1),
            ind(indent + 2),
            ind(indent + 1),
        )
    }

    /// One handler-map entry: the arm's implementation normalised to a binary
    /// `fun(Args, Handlers)` the dispatcher can invoke uniformly, passing the
    /// invocation-time map on iff the implementation is itself effectful.
    fn handler_entry(&self, handler: &IrExpr, env: &Env, cx: &mut FnCx, indent: usize) -> String {
        let args = cx.fresh_internal("Args");
        let map = cx.fresh_internal("Handlers");
        let effectful = self
            .effective_fn_type(handler, env)
            .is_some_and(|ty| matches!(&ty, Type::TyFn(_, _, row) if takes_map(row)));
        let map_arg = effectful.then(|| map.clone());
        let call = self.call_on(handler, Vec::from([args.clone()]), map_arg, env, cx, indent);
        let map_head = if effectful { map } else { format!("_{map}") };
        format!("fun({args}, {map_head}) -> {call} end")
    }

    /// A function application. Dispatches on the callee: primitive operators
    /// print infix, tool calls route through the runtime dispatcher, and
    /// everything else is a call whose handler-map argument and argument
    /// adaptations are read off the callee's function type.
    fn app(&self, app: &IrApp, env: &Env, cx: &mut FnCx, indent: usize) -> String {
        if let IrExpr::Var(v) = app.func.as_ref()
            && !env.scope.contains_key(&v.name)
        {
            if let Some(op) = erlang_operator(&v.name)
                && app.args.len() == 2
            {
                let lhs = self.operand(&app.args[0], env, cx, indent);
                let rhs = self.operand(&app.args[1], env, cx, indent);
                return format!("{lhs} {op} {rhs}");
            }
            if self.tools.contains_key(&v.name)
                && let [args_record] = app.args.as_slice()
            {
                let handlers = handlers_ref(env, cx);
                let args = self.expr(args_record, env, cx, indent, Ctx::Expr);
                return format!(
                    "hird_tool_dispatch:call({}, {}, {handlers}, {args})",
                    atom(&v.name),
                    caller_literal(env)
                );
            }
        }
        let fn_ty = self.effective_fn_type(&app.func, env);
        let (expected, callee_takes_map) = match &fn_ty {
            Some(Type::TyFn(params, _, row)) => (params.as_slice(), takes_map(row)),
            _ => (&[][..], false),
        };
        let args: Vec<String> = app
            .args
            .iter()
            .enumerate()
            .map(|(i, arg)| self.adapted_arg(arg, expected.get(i), env, cx, indent))
            .collect();
        let map_arg = callee_takes_map.then(|| handlers_ref(env, cx));
        self.call_on(&app.func, args, map_arg, env, cx, indent)
    }

    /// An operator operand, parenthesised when it is itself an operator
    /// application (all other forms are self-delimiting).
    fn operand(&self, expr: &IrExpr, env: &Env, cx: &mut FnCx, indent: usize) -> String {
        let rendered = self.expr(expr, env, cx, indent, Ctx::Expr);
        let nested_op = matches!(expr, IrExpr::App(a)
            if matches!(a.func.as_ref(), IrExpr::Var(v) if erlang_operator(&v.name).is_some()));
        if nested_op {
            format!("({rendered})")
        } else {
            rendered
        }
    }

    /// An argument, eta-expanded when a function value's own convention
    /// disagrees with the convention the callee's parameter type expects: a
    /// pure value at an effectful type grows a fun absorbing the ignored map;
    /// an effectful value at a pure type gets `#{}` (its instantiated row is
    /// empty, so the map is never consulted).
    fn adapted_arg(
        &self,
        arg: &IrExpr,
        expected: Option<&Type>,
        env: &Env,
        cx: &mut FnCx,
        indent: usize,
    ) -> String {
        let plain = |emitter: &Self, cx: &mut FnCx| emitter.expr(arg, env, cx, indent, Ctx::Expr);
        let Some(Type::TyFn(exp_params, _, exp_row)) = expected else {
            return plain(self, cx);
        };
        let Some(Type::TyFn(_, _, actual_row)) = self.effective_fn_type(arg, env) else {
            return plain(self, cx);
        };
        let expected_map = takes_map(exp_row);
        if expected_map == takes_map(&actual_row) {
            return plain(self, cx);
        }
        let vars: Vec<String> = exp_params.iter().map(|_| cx.fresh_internal("V")).collect();
        if expected_map {
            let call = self.call_on(arg, vars.clone(), None, env, cx, indent);
            let mut heads = vars;
            heads.push(String::from("_"));
            format!("fun({}) -> {call} end", heads.join(", "))
        } else {
            let call = self.call_on(
                arg,
                vars.clone(),
                Some(String::from("#{}")),
                env,
                cx,
                indent,
            );
            format!("fun({}) -> {call} end", vars.join(", "))
        }
    }

    /// A call of `callee` on already-rendered arguments (plus the optional
    /// trailing handler map): locals call through their variable, tools
    /// through the dispatcher, module-level and qualified names by atom
    /// (module-level names `remote`-qualified in an actor module), and any
    /// other callee expression parenthesised.
    fn call_on(
        &self,
        callee: &IrExpr,
        args: Vec<String>,
        map_arg: Option<String>,
        env: &Env,
        cx: &mut FnCx,
        indent: usize,
    ) -> String {
        if let IrExpr::Var(v) = callee {
            if let Some(binding) = env.scope.get(&v.name) {
                cx.referenced.insert(binding.var.clone());
                return format!("{}({})", binding.var, with_map(args, map_arg).join(", "));
            }
            if self.tools.contains_key(&v.name)
                && let [args_record] = args.as_slice()
            {
                let handlers = map_arg.unwrap_or_else(|| handlers_ref(env, cx));
                return format!(
                    "hird_tool_dispatch:call({}, {}, {handlers}, {args_record})",
                    atom(&v.name),
                    caller_literal(env)
                );
            }
            if let Some((module, member)) = v.name.rsplit_once('.') {
                return format!(
                    "{}:{}({})",
                    erlang_module_name(module),
                    atom(member),
                    with_map(args, map_arg).join(", ")
                );
            }
            if erlang_operator(&v.name).is_none() {
                let rendered_args = with_map(args, map_arg).join(", ");
                return match &self.remote {
                    Some(base) => format!("{base}:{}({rendered_args})", atom(&v.name)),
                    None => format!("{}({rendered_args})", atom(&v.name)),
                };
            }
        }
        let rendered = self.expr(callee, env, cx, indent, Ctx::Expr);
        format!("({rendered})({})", with_map(args, map_arg).join(", "))
    }

    /// A variable in value position: locals by their Erlang variable,
    /// module-level functions as `fun name/arity` (`remote`-qualified in an
    /// actor module), qualified names as remote fun references, and tools as
    /// a fun routing through the dispatcher.
    fn var_value(&self, v: &IrVar, env: &Env, cx: &mut FnCx) -> String {
        if let Some(binding) = env.scope.get(&v.name) {
            cx.referenced.insert(binding.var.clone());
            return binding.var.clone();
        }
        if self.tools.contains_key(&v.name) {
            let args = cx.fresh_internal("Args");
            let map = cx.fresh_internal("Handlers");
            return format!(
                "fun({args}, {map}) -> hird_tool_dispatch:call({}, {}, {map}, {args}) end",
                atom(&v.name),
                caller_literal(env)
            );
        }
        if let Some(ty) = self.fns.get(&v.name) {
            let arity = emitted_arity(ty);
            return match &self.remote {
                Some(base) => format!("fun {base}:{}/{arity}", atom(&v.name)),
                None => format!("fun {}/{arity}", atom(&v.name)),
            };
        }
        if let Some((module, member)) = v.name.rsplit_once('.') {
            let arity = emitted_arity(&v.ty);
            return format!(
                "fun {}:{}/{arity}",
                erlang_module_name(module),
                atom(member)
            );
        }
        // Unresolved (checked code should never produce this): fall back to
        // the variable spelling.
        variable_base(&v.name)
    }

    // ── function-type views ──────────────────────────────────────

    /// The function type governing `expr`'s calling convention, read from its
    /// *binding* rather than a use site's instantiation: a local's
    /// binding-site type, a module function's declared signature, a tool's
    /// derived signature, a lambda's own type, or a call's return type.
    /// `None` when the expression is not function-typed (or not statically a
    /// function).
    fn effective_fn_type(&self, expr: &IrExpr, env: &Env) -> Option<Type> {
        match expr {
            IrExpr::Var(v) => {
                if let Some(binding) = env.scope.get(&v.name) {
                    return as_fn(&binding.ty).cloned();
                }
                if let Some(ty) = self.tools.get(&v.name) {
                    return Some(ty.clone());
                }
                if let Some(ty) = self.fns.get(&v.name) {
                    return as_fn(ty).cloned();
                }
                as_fn(&v.ty).cloned()
            }
            IrExpr::Lambda(lambda) => Some(lambda_type(lambda)),
            IrExpr::App(app) => match self.effective_fn_type(&app.func, env) {
                Some(Type::TyFn(_, ret, _)) if matches!(ret.as_ref(), Type::TyFn(..)) => Some(*ret),
                _ => as_fn(&app.result_type).cloned(),
            },
            IrExpr::Let(le) => self.effective_fn_type(&le.body, env),
            other => as_fn(&expr_type(other)).cloned(),
        }
    }

    // ── patterns ─────────────────────────────────────────────────

    /// Renders a pattern, binding its variables (always fresh, so a pattern
    /// binder can never alias an outer Erlang variable) into `env`.
    fn pattern(&self, pattern: &IrPattern, env: &mut Env, cx: &mut FnCx) -> String {
        match pattern {
            IrPattern::Wildcard(_) => String::from("_"),
            IrPattern::Bind(bind) => {
                let var = cx.fresh_var(&bind.name);
                env.scope.insert(
                    bind.name.clone(),
                    Binding {
                        var: var.clone(),
                        ty: bind.ty.clone(),
                    },
                );
                var
            }
            IrPattern::Literal(lit) => literal(&lit.value),
            IrPattern::Tuple(tuple) => {
                if tuple.elems.is_empty() {
                    return String::from("ok");
                }
                let elems: Vec<String> = tuple
                    .elems
                    .iter()
                    .map(|p| self.pattern(p, env, cx))
                    .collect();
                format!("{{{}}}", elems.join(", "))
            }
            IrPattern::Constructor(ctor) => {
                let tag = atom(&snake_case(&ctor.name));
                if ctor.fields.is_empty() {
                    return tag;
                }
                let fields: Vec<String> = ctor
                    .fields
                    .iter()
                    .map(|p| self.pattern(p, env, cx))
                    .collect();
                format!("{{{tag}, {}}}", fields.join(", "))
            }
        }
    }
}

// ── free helpers ─────────────────────────────────────────────────

/// One `handle_call` clause source: the handler, its message constructor
/// pattern, and the `ReplyTo` field position within that constructor.
struct CallClause<'m> {
    /// The handler supplying the state pattern and body.
    handler: &'m IrActorHandler,
    /// The message constructor pattern.
    ctor: &'m IrConstructorPat,
    /// The `ReplyTo` field position in the constructor's declaration.
    reply_pos: usize,
}

/// Splits an actor's handlers into call clauses (message constructor carries
/// a `ReplyTo` field) and cast handlers, preserving declaration order.
fn split_handlers(actor: &IrActorDef) -> (Vec<CallClause<'_>>, Vec<&IrActorHandler>) {
    let mut calls = Vec::new();
    let mut casts = Vec::new();
    for handler in &actor.handlers {
        let call = match &handler.message {
            IrPattern::Constructor(ctor) => actor
                .message
                .constructors
                .iter()
                .find(|def| def.name == ctor.name)
                .and_then(|def| def.fields.iter().position(is_reply_to))
                .map(|reply_pos| CallClause {
                    handler,
                    ctor,
                    reply_pos,
                }),
            _ => None,
        };
        match call {
            Some(clause) => calls.push(clause),
            None => casts.push(handler),
        }
    }
    (calls, casts)
}

/// Whether `ty`'s head is the built-in `ReplyTo`.
fn is_reply_to(ty: &Type) -> bool {
    matches!(ty, Type::TyCon(name, _) if name.as_str() == "ReplyTo")
}

/// The single `init/1` argument carrying the init parameters: a lone
/// parameter travels bare; zero or several pack into a tuple.
fn init_arg(params: &[String]) -> String {
    match params {
        [single] => single.clone(),
        many => format!("{{{}}}", many.join(", ")),
    }
}

/// A literal's Erlang rendering: integers and floats verbatim, strings as
/// UTF-8 binaries (the escape repertoire is shared).
fn literal(value: &LiteralValue) -> String {
    match value {
        LiteralValue::Int(text) | LiteralValue::Float(text) => String::from(&**text),
        LiteralValue::Str(text) => {
            let inner = text
                .strip_prefix('"')
                .and_then(|t| t.strip_suffix('"'))
                .unwrap_or(text);
            format!("<<\"{inner}\"/utf8>>")
        }
    }
}

/// The handler-map key of an effect: `{tool, read_repo}`-style tuples for
/// parametric effects, the snake-cased head atom for bare ones.
fn effect_key(effect: &Effect) -> String {
    let head = atom(&snake_case(effect.head().as_str()));
    if effect.args().is_empty() {
        return head;
    }
    let args: Vec<String> = effect.args().iter().map(type_atom).collect();
    format!("{{{head}, {}}}", args.join(", "))
}

/// The atom naming a type argument in an effect key (`ReadRepo` →
/// `read_repo`); a non-constructor argument falls back to its rendering.
fn type_atom(ty: &Type) -> String {
    match ty {
        Type::TyCon(name, _) => atom(&snake_case(name.as_str())),
        other => atom(&snake_case(&format!("{other}"))),
    }
}

/// The caller id injected at a dispatch site, as an Erlang binary literal.
/// Caller ids are built from Hirð identifiers, `.`, and `/`, none of which
/// need escaping inside a string literal.
fn caller_literal(env: &Env) -> String {
    format!("<<\"{}\"/utf8>>", env.caller)
}

/// The in-scope handler map, or the empty map in a pure context.
fn handlers_ref(env: &Env, cx: &mut FnCx) -> String {
    match &env.handlers {
        Some(var) => {
            cx.referenced.insert(var.clone());
            var.clone()
        }
        None => String::from("#{}"),
    }
}

/// `args` with the optional trailing handler map appended.
fn with_map(mut args: Vec<String>, map_arg: Option<String>) -> Vec<String> {
    if let Some(map) = map_arg {
        args.push(map);
    }
    args
}

/// Joins already-rendered statements into a body-position sequence, or wraps
/// them in `begin … end` in expression position. Statements other than the
/// last must carry their own trailing comma.
fn sequence(stmts: &[String], indent: usize, ctx: Ctx) -> String {
    match ctx {
        Ctx::Body => stmts.join(&format!("\n{}", ind(indent))),
        Ctx::Expr => format!(
            "begin\n{}{}\n{}end",
            ind(indent + 1),
            stmts.join(&format!("\n{}", ind(indent + 1))),
            ind(indent)
        ),
    }
}

/// The Erlang spelling of a primitive binary operator, `None` for names that
/// are not operators. `/` is integer division per the v0.1 operator table;
/// equality is the exact `=:=`/`=/=` (operands are same-typed already).
fn erlang_operator(name: &str) -> Option<&'static str> {
    Some(match name {
        "+" => "+",
        "-" => "-",
        "*" => "*",
        "/" => "div",
        "==" => "=:=",
        "!=" => "=/=",
        "<" => "<",
        "<=" => "=<",
        ">" => ">",
        ">=" => ">=",
        "\u{2227}" => "andalso",
        "\u{2228}" => "orelse",
        _ => return None,
    })
}

/// A function definition's type: parameters, return, and declared row.
fn fn_def_type(f: &IrFnDef) -> Type {
    Type::TyFn(
        f.params.iter().map(|p| p.ty.clone()).collect(),
        Box::new(f.return_type.clone()),
        f.effect_row.clone(),
    )
}

/// A lambda's own function type.
fn lambda_type(lambda: &IrLambda) -> Type {
    Type::TyFn(
        lambda.params.iter().map(|p| p.ty.clone()).collect(),
        Box::new(lambda.body_type.clone()),
        lambda.effect_row.clone(),
    )
}

/// `ty` with any outer quantifier stripped.
fn unquantified(ty: &Type) -> &Type {
    match ty {
        Type::TyForall(_, _, body) => body,
        other => other,
    }
}

/// `ty` as a function type, looking through quantifiers.
fn as_fn(ty: &Type) -> Option<&Type> {
    match unquantified(ty) {
        fn_ty @ Type::TyFn(..) => Some(fn_ty),
        _ => None,
    }
}

/// The arity a function of type `ty` is emitted at: surface arity plus one
/// for the handler map when the row asks for it.
fn emitted_arity(ty: &Type) -> usize {
    match unquantified(ty) {
        Type::TyFn(params, _, row) => params.len() + usize::from(takes_map(row)),
        _ => 0,
    }
}

/// The type an expression's value has, read off the node (bodies looked
/// through for `let`).
fn expr_type(expr: &IrExpr) -> Type {
    match expr {
        IrExpr::Literal(lit) => lit.ty.clone(),
        IrExpr::Var(v) => v.ty.clone(),
        IrExpr::Let(le) => expr_type(&le.body),
        IrExpr::Lambda(lambda) => lambda_type(lambda),
        IrExpr::App(app) => app.result_type.clone(),
        IrExpr::Match(m) => m.result_type.clone(),
        IrExpr::Handle(h) => h.result_type.clone(),
        IrExpr::Install(inst) => inst.result_type.clone(),
        IrExpr::Spawn(s) => s.result_type.clone(),
        IrExpr::Supervise(s) => s.result_type.clone(),
        IrExpr::Stand(s) => s.result_type.clone(),
        IrExpr::Child(c) => c.result_type.clone(),
        IrExpr::Send(s) => s.result_type.clone(),
        IrExpr::Request(r) => r.result_type.clone(),
        IrExpr::Reply(r) => r.result_type.clone(),
        IrExpr::Crash(c) => c.result_type.clone(),
        IrExpr::Constructor(c) => c.result_type.clone(),
        IrExpr::Tuple(t) => t.ty.clone(),
        IrExpr::List(l) => l.ty.clone(),
        IrExpr::Record(r) => r.ty.clone(),
        IrExpr::Field(f) => f.ty.clone(),
    }
}

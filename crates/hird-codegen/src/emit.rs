// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Erlang source emission: one IR module to one `.erl` file.
//!
//! The output compiles with stock `erlc` and is formatted for human reading.
//! Types are erased (no forms for type, tool, actor, or supervisor
//! declarations — actor and supervisor behaviour modules are emitted
//! separately); functions and extern stubs are the emitted forms, each headed
//! by a `%% <file>:<line>` comment from its declaration span.
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
//!   `hird_tool_dispatch:call(tool_name, Handlers, Args)` — never a direct
//!   handler invocation — so audit capture is unconditional.
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

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_ir::{
    IrApp, IrConstructor, IrDecl, IrExpr, IrExternRef, IrFnDef, IrHandle, IrLambda, IrLet, IrMatch,
    IrModule, IrPattern, IrSpan, IrVar, LiteralValue,
};
use hird_types::{Effect, EffectRow, Type};

use crate::names::{atom, erlang_module_name, snake_case, variable_base};

/// Renders `module` as the text of one Erlang source file.
///
/// `source_path` is the Hirð source the module was lowered from; it appears
/// in the generated-file banner and in the `%% <file>:<line>` comment above
/// each form. The output's module name is [`erlang_module_name`] of the
/// module's name, so the caller should write it to `<that name>.erl`.
#[must_use]
pub fn emit_module(module: &IrModule, source_path: &str) -> String {
    let emitter = Emitter::new(module);
    emitter.module(source_path)
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

/// The lexical environment: local bindings and the in-scope handler map.
#[derive(Clone, Default)]
struct Env {
    /// Hirð name → binding, innermost shadowing outermost.
    scope: BTreeMap<String, Binding>,
    /// The in-scope handler-map variable; `None` in a pure context, where a
    /// needed map falls back to `#{}`.
    handlers: Option<String>,
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
    /// Module-level function or extern name → its function type (declared
    /// parameter and row, not a use site's instantiation).
    fns: BTreeMap<String, Type>,
}

impl<'a> Emitter<'a> {
    /// Builds the tool and function tables for `module`.
    fn new(module: &'a IrModule) -> Self {
        let mut tools = BTreeMap::new();
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
                }
                IrDecl::Type(_) | IrDecl::Actor(_) | IrDecl::Supervisor(_) => {}
            }
        }
        Self { module, tools, fns }
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
                // sites; actor and supervisor behaviour modules are emitted
                // separately.
                IrDecl::Type(_) | IrDecl::Tool(_) | IrDecl::Actor(_) | IrDecl::Supervisor(_) => {}
            }
        }
        out
    }

    /// The export list: every function and extern at its emitted arity
    /// (surface arity plus one for the handler map when the row asks for it).
    fn exports(&self) -> Vec<String> {
        self.module
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
            .collect()
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
        let mut env = Env::default();
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
                    "hird_tool_dispatch:call({}, {handlers}, {args})",
                    atom(&v.name)
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
    /// through the dispatcher, module-level and qualified names by atom, and
    /// any other callee expression parenthesised.
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
                    "hird_tool_dispatch:call({}, {handlers}, {args_record})",
                    atom(&v.name)
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
                return format!("{}({})", atom(&v.name), with_map(args, map_arg).join(", "));
            }
        }
        let rendered = self.expr(callee, env, cx, indent, Ctx::Expr);
        format!("({rendered})({})", with_map(args, map_arg).join(", "))
    }

    /// A variable in value position: locals by their Erlang variable,
    /// module-level functions as `fun name/arity`, qualified names as remote
    /// fun references, and tools as a fun routing through the dispatcher.
    fn var_value(&self, v: &IrVar, env: &Env, cx: &mut FnCx) -> String {
        if let Some(binding) = env.scope.get(&v.name) {
            cx.referenced.insert(binding.var.clone());
            return binding.var.clone();
        }
        if self.tools.contains_key(&v.name) {
            let args = cx.fresh_internal("Args");
            let map = cx.fresh_internal("Handlers");
            return format!(
                "fun({args}, {map}) -> hird_tool_dispatch:call({}, {map}, {args}) end",
                atom(&v.name)
            );
        }
        if let Some(ty) = self.fns.get(&v.name) {
            return format!("fun {}/{}", atom(&v.name), emitted_arity(ty));
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
        IrExpr::Spawn(s) => s.result_type.clone(),
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

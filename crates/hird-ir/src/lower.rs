// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Lowering from the typed AST to the IR.
//!
//! The checker leaves a resolved type on every visited node (keyed by CST
//! identity in [`CheckedFile`]). Lowering walks the same CST through the
//! [`hird_ast`] projection, reads those resolved types back, and emits fully
//! typed IR. No substitution happens here: the checker already applied it.
//!
//! Desugaring is intentional and documented:
//!
//! - `if c then a else b` becomes `match c { True → a, False → b }`.
//! - Binary operators become application of a primitive operator reference.
//! - Parentheses are dropped (they carry no semantics).
//! - `handle { … } in body` lowers to an [`IrHandle`]
//!   carrying the handler arms, the body, and the block's computed effect row;
//!   the checker resolves the handled effect of each arm and the row.
//!
//! Functions and applications are n-ary, matching the type system: `f(a, b)`
//! is a two-argument call, not a curried chain.
//!
//! The input must be a parse-error-free, type-error-free [`CheckedFile`].
//! Lowering reads the types the checker recorded; a missing entry is an
//! internal invariant violation and panics.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_ast::{
    ActorDecl, ActorField, ActorHandler, AppExpr, AstNode, BinOpExpr, ChildExpr, CrashExpr, Decl,
    Expr, ExternDecl, FieldExpr, FnDecl, HandleBlock, IfExpr, InstallBlock, LambdaExpr, LetExpr,
    Literal, MatchExpr, Pattern, RecordField, RecordLit, ReplyExpr, RequestExpr, ScheduleExpr,
    SendExpr, SeqExpr, SourceFile, SpawnExpr, SuperviseExpr, SupervisorDecl, SupervisorField,
    ToolDecl, TupleLit, TypeDecl,
};
use hird_check::{CheckedFile, NodeKey};
use hird_parse::SyntaxKind;
use hird_types::{Effect, EffectRow, Type};

use crate::ir::{
    IrActorDef, IrActorHandler, IrActorInit, IrApp, IrArm, IrBindPat, IrChild, IrChildSpec,
    IrClock, IrConstructor, IrConstructorDef, IrConstructorPat, IrCrash, IrDecl, IrExpr,
    IrExternRef, IrField, IrFnDef, IrHandle, IrHandleArm, IrInstall, IrLambda, IrLet, IrList,
    IrLiteral, IrLiteralPat, IrMatch, IrModule, IrParam, IrPattern, IrRecord, IrRecordField,
    IrReply, IrRequest, IrSchedule, IrSelf, IrSend, IrSpan, IrSpawn, IrStand, IrSupervise,
    IrSupervisorDef, IrToolDef, IrTuple, IrTuplePat, IrTypeDef, IrVar, IrWildcardPat, LiteralValue,
};

/// Lowers one checked module into IR.
///
/// `file` is the parsed source, `checked` its check result (the source of all
/// node types), and `name` the module's authoritative name. Declarations that
/// the checker did not type (parser-recovery artefacts) are skipped.
///
/// # Panics
///
/// Panics if `checked` lacks a recorded type for a node lowering visits, which
/// only happens when `file`/`checked` disagree or the input was not
/// error-free.
#[must_use]
pub fn lower_module(file: &SourceFile, checked: &CheckedFile, name: &str) -> IrModule {
    let lowerer = Lowerer {
        checked,
        newlines: newline_offsets(file),
    };
    let mut declarations = Vec::new();
    for decl in file.declarations() {
        match decl {
            Decl::Fn(d) => declarations.extend(lowerer.lower_fn(&d).map(IrDecl::Fn)),
            Decl::Type(d) => declarations.extend(lowerer.lower_type(&d).map(IrDecl::Type)),
            Decl::Extern(d) => declarations.extend(lowerer.lower_extern(&d).map(IrDecl::Extern)),
            Decl::Tool(d) => declarations.extend(lowerer.lower_tool(&d).map(IrDecl::Tool)),
            Decl::Actor(d) => declarations.extend(lowerer.lower_actor(&d).map(IrDecl::Actor)),
            Decl::Supervisor(d) => {
                declarations.extend(lowerer.lower_supervisor(&d).map(IrDecl::Supervisor));
            }
            // Imports are resolved away, and effect declarations are
            // synthesised on printing rather than lowered.
            _ => {}
        }
    }
    IrModule {
        name: String::from(name),
        declarations,
    }
}

/// Carries the checked file whose recorded types lowering reads back.
struct Lowerer<'a> {
    /// The check result: resolved types keyed by CST identity, plus the ADT
    /// table.
    checked: &'a CheckedFile,
    /// Byte offsets of the source's newlines, ascending, for span lines.
    newlines: Vec<u32>,
}

impl Lowerer<'_> {
    // ── declarations ─────────────────────────────────────────────

    /// Lowers a function declaration. `None` when the declaration is missing a
    /// name or body (parser recovery).
    fn lower_fn(&self, decl: &FnDecl) -> Option<IrFnDef> {
        let name = decl.name()?;
        let body = decl.body()?;
        let params = decl
            .params()
            .map(|p| IrParam {
                name: String::from(p.name().unwrap_or("")),
                ty: self.node_type(p.syntax()),
            })
            .collect();
        let return_type = self.expr_type(&body);
        // The checker records each function's declared effect row, resolved
        // against the same elaboration as the parameter types so shared row
        // variables keep one identity; an un-annotated function has no entry
        // and carries the empty row.
        let effect_row = self
            .checked
            .effect_row_at(NodeKey::of_node(decl.syntax()))
            .cloned()
            .unwrap_or_default();
        Some(IrFnDef {
            name: String::from(name),
            params,
            return_type,
            effect_row,
            body: self.lower_expr(&body),
            span: self.span(decl.syntax()),
        })
    }

    /// Lowers a data type declaration. `None` when it is missing a name.
    fn lower_type(&self, decl: &TypeDecl) -> Option<IrTypeDef> {
        let name = decl.name()?;
        let params: Vec<String> = decl.type_params().map(String::from).collect();
        let constructors = decl
            .constructors()
            .filter_map(|ctor| {
                let ctor_name = ctor.name()?;
                let scheme = self.checked.type_at(NodeKey::of_node(ctor.syntax()))?;
                Some(IrConstructorDef {
                    name: String::from(ctor_name),
                    fields: constructor_field_types(scheme, &params),
                })
            })
            .collect();
        Some(IrTypeDef {
            name: String::from(name),
            params,
            constructors,
            span: self.span(decl.syntax()),
        })
    }

    /// Lowers a tool declaration. `None` when it is missing a name or the
    /// checker did not record its generated function's scheme.
    fn lower_tool(&self, decl: &ToolDecl) -> Option<IrToolDef> {
        let name = decl.name()?;
        let scheme = self.checked.type_at(NodeKey::of_node(decl.syntax()))?;
        let params: Vec<String> = decl.type_params().map(String::from).collect();
        let (input, output, effect_row) = tool_signature(scheme, &params, name)?;
        Some(IrToolDef {
            name: String::from(name),
            params,
            input,
            output,
            effect_row,
            span: self.span(decl.syntax()),
        })
    }

    /// Lowers an actor declaration. `None` when the declaration is missing a
    /// member (parser recovery or a reported structure error).
    fn lower_actor(&self, decl: &ActorDecl) -> Option<IrActorDef> {
        let name = decl.name()?;
        let state_field = actor_field(decl, "state")?;
        let message_field = actor_field(decl, "message")?;
        let init_field = actor_field(decl, "init")?;

        let state = self
            .checked
            .type_at(NodeKey::of_node(state_field.syntax()))?
            .clone();
        let message = self.lower_actor_message(&message_field)?;
        let init = self.lower_actor_init(&init_field)?;
        let handlers = decl
            .handlers()
            .filter_map(|h| self.lower_actor_handler(&h))
            .collect();
        let effect_row = self
            .checked
            .effect_row_at(NodeKey::of_node(decl.syntax()))
            .cloned()
            .unwrap_or_default();
        Some(IrActorDef {
            name: String::from(name),
            state,
            message,
            init,
            handlers,
            effect_row,
            span: self.span(decl.syntax()),
        })
    }

    /// Lowers an actor's message field to the sum type it declares.
    fn lower_actor_message(&self, field: &ActorField) -> Option<IrTypeDef> {
        let hird_ast::TypeExpr::Name(name) = field.ty()? else {
            return None;
        };
        let constructors = field
            .constructors()
            .filter_map(|ctor| {
                let ctor_name = ctor.name()?;
                let scheme = self.checked.type_at(NodeKey::of_node(ctor.syntax()))?;
                Some(IrConstructorDef {
                    name: String::from(ctor_name),
                    fields: constructor_field_types(scheme, &[]),
                })
            })
            .collect();
        Some(IrTypeDef {
            name: String::from(name.text()),
            params: Vec::new(),
            constructors,
            span: self.span(field.syntax()),
        })
    }

    /// Lowers an actor's init field: parameters from the signature, the
    /// declared row, and the body.
    fn lower_actor_init(&self, field: &ActorField) -> Option<IrActorInit> {
        let sig = field.fn_sig()?;
        let body = field.body()?;
        let params = sig
            .params()
            .map(|p| IrParam {
                name: String::from(p.name().unwrap_or("")),
                ty: self.node_type(p.syntax()),
            })
            .collect();
        let effect_row = self
            .checked
            .effect_row_at(NodeKey::of_node(sig.syntax()))
            .cloned()
            .unwrap_or_default();
        Some(IrActorInit {
            params,
            effect_row,
            body: self.lower_expr(&body),
        })
    }

    /// Lowers one `handle` clause: the message and state patterns, the
    /// declared row, and the body.
    fn lower_actor_handler(&self, handler: &ActorHandler) -> Option<IrActorHandler> {
        let message = handler.message_pattern()?;
        let state = handler.state_pattern()?;
        let body = handler.body()?;
        let effect_row = self
            .checked
            .effect_row_at(NodeKey::of_node(handler.syntax()))
            .cloned()
            .unwrap_or_default();
        Some(IrActorHandler {
            message: self.lower_pattern(&message),
            state: self.lower_pattern(&state),
            effect_row,
            body: self.lower_expr(&body),
        })
    }

    /// Lowers a supervisor declaration. `None` when a required field is missing
    /// or malformed (a reported structure error). Field validity is the
    /// checker's job; lowering trusts a checked declaration and reads its
    /// derived effect row back from the check result.
    fn lower_supervisor(&self, decl: &SupervisorDecl) -> Option<IrSupervisorDef> {
        let name = decl.name()?;
        let strategy = supervisor_ident(decl, "strategy")?;
        let intensity = supervisor_int(decl, "intensity")?;
        let period = supervisor_int(decl, "period")?;
        let children = self.lower_children(decl)?;
        let effect_row = self
            .checked
            .effect_row_at(NodeKey::of_node(decl.syntax()))
            .cloned()
            .unwrap_or_default();
        Some(IrSupervisorDef {
            name: String::from(name),
            strategy,
            intensity,
            period,
            children,
            effect_row,
            span: self.span(decl.syntax()),
        })
    }

    /// Lowers a supervisor's `children` list, skipping malformed specs.
    fn lower_children(&self, decl: &SupervisorDecl) -> Option<Vec<IrChildSpec>> {
        let Expr::List(list) = supervisor_field(decl, "children")?.value()? else {
            return None;
        };
        let children = list
            .elements()
            .filter_map(|elem| match elem {
                Expr::Record(spec) => self.lower_child(&spec),
                _ => None,
            })
            .collect();
        Some(children)
    }

    /// Lowers one child spec: the identifier fields verbatim and the
    /// `start_args` expression through the ordinary expression lowering.
    fn lower_child(&self, spec: &RecordLit) -> Option<IrChildSpec> {
        let start_args = record_field(spec, "start_args")?.value()?;
        Some(IrChildSpec {
            id: record_ident(spec, "id")?,
            actor: record_ident(spec, "actor")?,
            start_args: self.lower_expr(&start_args),
            restart: record_ident(spec, "restart")?,
        })
    }

    /// Lowers an extern declaration. `None` when it is missing a name or the
    /// checker did not record its scheme.
    fn lower_extern(&self, decl: &ExternDecl) -> Option<IrExternRef> {
        let name = decl.name()?;
        let ty = self
            .checked
            .type_at(NodeKey::of_node(decl.syntax()))?
            .clone();
        Some(IrExternRef {
            name: String::from(name),
            ty,
            // The surface syntax does not yet name a backing FFI module.
            module: None,
            span: self.span(decl.syntax()),
        })
    }

    // ── expressions ──────────────────────────────────────────────

    /// Lowers an expression to IR.
    fn lower_expr(&self, expr: &Expr) -> IrExpr {
        match expr {
            Expr::Literal(lit) => IrExpr::Literal(IrLiteral {
                value: literal_value(lit),
                ty: self.expr_type(expr),
            }),
            // A use of an unqualified imported function is qualified to its
            // defining module, the form remote calls already lower through.
            Expr::Name(name) => match self.checked.import_origins.get(&NodeKey::of_expr(expr)) {
                Some(from) => IrExpr::Var(IrVar {
                    name: format!("{from}.{}", name.text()),
                    ty: self.expr_type(expr),
                }),
                None => self.lower_name(name.text(), self.expr_type(expr)),
            },
            Expr::Let(le) => self.lower_let(le),
            Expr::Seq(seq) => self.lower_seq(seq),
            Expr::Lambda(lambda) => self.lower_lambda(lambda),
            Expr::If(ife) => self.lower_if(ife),
            Expr::Match(me) => self.lower_match(me),
            Expr::Handle(handle) => self.lower_handle(handle),
            Expr::Install(install) => self.lower_install(install),
            Expr::Spawn(spawn) => self.lower_spawn(spawn),
            Expr::Supervise(supervise) => self.lower_supervise(supervise),
            Expr::Stand(stand) => IrExpr::Stand(IrStand {
                result_type: self.node_type(stand.syntax()),
            }),
            Expr::Clock(clock) => IrExpr::Clock(IrClock {
                result_type: self.node_type(clock.syntax()),
            }),
            Expr::SelfRef(this) => IrExpr::SelfRef(IrSelf {
                result_type: self.node_type(this.syntax()),
            }),
            Expr::Schedule(schedule) => self.lower_schedule(schedule),
            Expr::Child(child) => self.lower_child_lookup(child),
            Expr::Send(send) => self.lower_send(send),
            Expr::Request(request) => self.lower_request(request),
            Expr::Reply(reply) => self.lower_reply(reply),
            Expr::Crash(crash) => self.lower_crash(crash),
            Expr::BinOp(op) => self.lower_binop(op),
            Expr::App(app) => self.lower_app(app),
            Expr::Field(field) => self.lower_field(field),
            Expr::Record(record) => self.lower_record(record),
            Expr::Tuple(tuple) => self.lower_tuple(tuple),
            Expr::List(list) => IrExpr::List(IrList {
                elems: list.elements().map(|e| self.lower_expr(&e)).collect(),
                ty: self.expr_type(expr),
            }),
            Expr::Paren(paren) => match paren.inner() {
                Some(inner) => self.lower_expr(&inner),
                None => self.unit(),
            },
        }
    }

    /// Lowers a bare name: a `PascalCase` name is a nullary constructor, any
    /// other a variable.
    fn lower_name(&self, text: &str, ty: Type) -> IrExpr {
        if is_constructor(text) {
            IrExpr::Constructor(IrConstructor {
                name: String::from(text),
                type_name: head_type_name(&ty).unwrap_or_else(|| String::from(text)),
                args: Vec::new(),
                result_type: ty,
            })
        } else {
            IrExpr::Var(IrVar {
                name: String::from(text),
                ty,
            })
        }
    }

    /// `let name = value in body`. The binding's recorded type is the bound
    /// value's type.
    fn lower_let(&self, le: &LetExpr) -> IrExpr {
        let pattern = le.pattern().expect("let has a binder");
        let value = le.value().expect("let has a value");
        let body = le.body().expect("let has a body");
        match &pattern {
            Pattern::Bind(bind) => IrExpr::Let(IrLet {
                name: String::from(bind.name().unwrap_or("")),
                ty: self.expr_type(&value),
                value: Box::new(self.lower_expr(&value)),
                body: Box::new(self.lower_expr(&body)),
            }),
            // A wildcard binder discards the value: a let named `_`, which
            // codegen emits as `_ = Value`, so sequencing an effect costs no
            // `case` and no invented name.
            Pattern::Wildcard(_) => IrExpr::Let(IrLet {
                name: String::from("_"),
                ty: self.expr_type(&value),
                value: Box::new(self.lower_expr(&value)),
                body: Box::new(self.lower_expr(&body)),
            }),
            // Any other destructuring binder is a one-arm match: the IR has no
            // pattern-let node, and the checker has proved the arm total.
            _ => IrExpr::Match(IrMatch {
                scrutinee_type: self.expr_type(&value),
                scrutinee: Box::new(self.lower_expr(&value)),
                arms: Vec::from([IrArm {
                    pattern: self.lower_pattern(&pattern),
                    body: self.lower_expr(&body),
                }]),
                result_type: self.expr_type(&body),
            }),
        }
    }

    /// `first; rest` is the discard let it abbreviates: a let named `_`, which
    /// codegen emits as `_ = First`.
    fn lower_seq(&self, seq: &SeqExpr) -> IrExpr {
        let first = seq.first().expect("sequence has a first expression");
        let rest = seq.rest().expect("sequence has a rest expression");
        IrExpr::Let(IrLet {
            name: String::from("_"),
            ty: self.expr_type(&first),
            value: Box::new(self.lower_expr(&first)),
            body: Box::new(self.lower_expr(&rest)),
        })
    }

    /// `λparams → body`. Parameter types and the effect row come from the
    /// lambda's own function type, so each parameter is explicitly typed and
    /// the calling convention is readable off the node.
    fn lower_lambda(&self, lambda: &LambdaExpr) -> IrExpr {
        let (param_tys, body_type, effect_row) = match self.node_type(lambda.syntax()) {
            Type::TyFn(params, ret, row) => (params, *ret, row),
            other => (Vec::new(), other, EffectRow::empty()),
        };
        let params = lambda
            .param_names()
            .zip(param_tys)
            .map(|(name, ty)| IrParam {
                name: String::from(name),
                ty,
            })
            .collect();
        let body = lambda.body().expect("lambda has a body");
        IrExpr::Lambda(IrLambda {
            params,
            body: Box::new(self.lower_expr(&body)),
            body_type,
            effect_row,
        })
    }

    /// `if c then a else b` desugars to `match c { True → a, False → b }`.
    fn lower_if(&self, ife: &IfExpr) -> IrExpr {
        let cond = ife.condition().expect("if has a condition");
        let then_branch = ife.then_branch().expect("if has a then-branch");
        let else_branch = ife.else_branch().expect("if has an else-branch");
        let result_type = self.node_type(ife.syntax());
        let arms = Vec::from([
            IrArm {
                pattern: bool_pattern("True"),
                body: self.lower_expr(&then_branch),
            },
            IrArm {
                pattern: bool_pattern("False"),
                body: self.lower_expr(&else_branch),
            },
        ]);
        IrExpr::Match(IrMatch {
            scrutinee: Box::new(self.lower_expr(&cond)),
            scrutinee_type: Type::bool(),
            arms,
            result_type,
        })
    }

    /// `match scrutinee { arms }`.
    fn lower_match(&self, me: &MatchExpr) -> IrExpr {
        let scrutinee = me.scrutinee().expect("match has a scrutinee");
        let arms = me
            .arms()
            .filter_map(|arm| {
                let pattern = arm.pattern()?;
                let body = arm.body()?;
                Some(IrArm {
                    pattern: self.lower_pattern(&pattern),
                    body: self.lower_expr(&body),
                })
            })
            .collect();
        IrExpr::Match(IrMatch {
            scrutinee_type: self.expr_type(&scrutinee),
            scrutinee: Box::new(self.lower_expr(&scrutinee)),
            arms,
            result_type: self.node_type(me.syntax()),
        })
    }

    /// `handle { effect → handler, … } in body`. The handled effect of each arm
    /// and the block's row come from the checker's side-tables.
    fn lower_handle(&self, handle: &HandleBlock) -> IrExpr {
        let body = handle.body().expect("handle has a body");
        let arms = handle
            .arms()
            .filter_map(|arm| {
                let effect = self
                    .checked
                    .handled_effect_at(NodeKey::of_node(arm.syntax()))?
                    .clone();
                let handler = arm.handler()?;
                Some(IrHandleArm {
                    effect,
                    handler: self.lower_expr(&handler),
                })
            })
            .collect();
        let effect_row = self
            .checked
            .effect_row_at(NodeKey::of_node(handle.syntax()))
            .cloned()
            .unwrap_or_default();
        IrExpr::Handle(IrHandle {
            arms,
            body: Box::new(self.lower_expr(&body)),
            effect_row,
            result_type: self.node_type(handle.syntax()),
        })
    }

    /// `install { effect → handler, … } in body`. The arms and the block's row
    /// come from the same checker side-tables as a `handle` block's.
    fn lower_install(&self, install: &InstallBlock) -> IrExpr {
        let body = install.body().expect("install has a body");
        let arms = install
            .arms()
            .filter_map(|arm| {
                let effect = self
                    .checked
                    .handled_effect_at(NodeKey::of_node(arm.syntax()))?
                    .clone();
                let handler = arm.handler()?;
                Some(IrHandleArm {
                    effect,
                    handler: self.lower_expr(&handler),
                })
            })
            .collect();
        let effect_row = self
            .checked
            .effect_row_at(NodeKey::of_node(install.syntax()))
            .cloned()
            .unwrap_or_default();
        IrExpr::Install(IrInstall {
            arms,
            body: Box::new(self.lower_expr(&body)),
            effect_row,
            result_type: self.node_type(install.syntax()),
        })
    }

    /// `spawn(Actor, args…)`. The actor name is carried as a string — it is a
    /// namespace reference, not an expression — and the recorded type is the
    /// typed `Pid<Msg>` reference.
    fn lower_spawn(&self, spawn: &SpawnExpr) -> IrExpr {
        IrExpr::Spawn(IrSpawn {
            actor: String::from(spawn.actor_name().unwrap_or("")),
            args: spawn.args().map(|a| self.lower_expr(&a)).collect(),
            result_type: self.node_type(spawn.syntax()),
        })
    }

    /// `supervise(SupName)`. The supervisor name is carried as a string — it
    /// is a namespace reference, not an expression — and the recorded type is
    /// unit.
    fn lower_supervise(&self, supervise: &SuperviseExpr) -> IrExpr {
        IrExpr::Supervise(IrSupervise {
            supervisor: String::from(supervise.supervisor_name().unwrap_or("")),
            result_type: self.node_type(supervise.syntax()),
        })
    }

    /// `child(SupName, child_id)`. Both names are carried as strings — they
    /// are namespace references, not expressions — and the recorded type is
    /// the typed `Pid<Msg>` reference.
    fn lower_child_lookup(&self, child: &ChildExpr) -> IrExpr {
        IrExpr::Child(IrChild {
            supervisor: String::from(child.supervisor_name().unwrap_or("")),
            child_id: String::from(child.child_id().unwrap_or("")),
            result_type: self.node_type(child.syntax()),
        })
    }

    /// `send(pid, msg)`. The recorded type is unit.
    fn lower_send(&self, send: &SendExpr) -> IrExpr {
        let pid = send.pid().expect("send has a pid");
        let message = send.message().expect("send has a message");
        IrExpr::Send(IrSend {
            pid: Box::new(self.lower_expr(&pid)),
            message: Box::new(self.lower_expr(&message)),
            result_type: self.node_type(send.syntax()),
        })
    }

    /// `request(pid, ctor[, timeout_ms])`. The recorded type is the reply
    /// type.
    fn lower_request(&self, request: &RequestExpr) -> IrExpr {
        let pid = request.pid().expect("request has a pid");
        let message_fn = request.message_fn().expect("request has a message builder");
        IrExpr::Request(IrRequest {
            pid: Box::new(self.lower_expr(&pid)),
            message_fn: Box::new(self.lower_expr(&message_fn)),
            timeout: request
                .timeout()
                .map(|timeout| Box::new(self.lower_expr(&timeout))),
            result_type: self.node_type(request.syntax()),
        })
    }

    /// `schedule(clock, pid, msg, delay_ms)`. The recorded type is unit.
    fn lower_schedule(&self, schedule: &ScheduleExpr) -> IrExpr {
        let clock = schedule.clock().expect("schedule has a clock");
        let pid = schedule.pid().expect("schedule has a pid");
        let message = schedule.message().expect("schedule has a message");
        let delay = schedule.delay().expect("schedule has a delay");
        IrExpr::Schedule(IrSchedule {
            clock: Box::new(self.lower_expr(&clock)),
            pid: Box::new(self.lower_expr(&pid)),
            message: Box::new(self.lower_expr(&message)),
            delay: Box::new(self.lower_expr(&delay)),
            result_type: self.node_type(schedule.syntax()),
        })
    }

    /// `reply(reply_to, value)`. The recorded type is unit.
    fn lower_reply(&self, reply: &ReplyExpr) -> IrExpr {
        let reply_to = reply.reply_to().expect("reply has a reply channel");
        let value = reply.value().expect("reply has a value");
        IrExpr::Reply(IrReply {
            reply_to: Box::new(self.lower_expr(&reply_to)),
            value: Box::new(self.lower_expr(&value)),
            result_type: self.node_type(reply.syntax()),
        })
    }

    /// `crash!(message)` / `panic!(message)`. The recorded type is the fresh
    /// variable the checker unified with this use site's context; `panic!`
    /// lowers identically (the surface alias is not preserved).
    fn lower_crash(&self, crash: &CrashExpr) -> IrExpr {
        let message = crash.message().expect("crash has a message");
        IrExpr::Crash(IrCrash {
            message: Box::new(self.lower_expr(&message)),
            result_type: self.node_type(crash.syntax()),
        })
    }

    /// `a ⊕ b` desugars to application of the operator's primitive reference.
    fn lower_binop(&self, op: &BinOpExpr) -> IrExpr {
        let lhs = op.lhs().expect("binop has a left operand");
        let rhs = op.rhs().expect("binop has a right operand");
        let result_type = self.node_type(op.syntax());
        let lhs_ty = self.expr_type(&lhs);
        let rhs_ty = self.expr_type(&rhs);
        let op_name = canonical_operator(op.op().expect("binop has an operator"));
        let func = IrExpr::Var(IrVar {
            name: op_name,
            ty: Type::func(Vec::from([lhs_ty, rhs_ty]), result_type.clone()),
        });
        IrExpr::App(IrApp {
            func: Box::new(func),
            args: Vec::from([self.lower_expr(&lhs), self.lower_expr(&rhs)]),
            result_type,
        })
    }

    /// `func(args)`. A `PascalCase` callee is a constructor application; any
    /// other is an ordinary call. The argument shape follows the checker: a
    /// tuple-literal argument is the argument list, anything else is a single
    /// argument.
    fn lower_app(&self, app: &AppExpr) -> IrExpr {
        let result_type = self.node_type(app.syntax());
        let arg_exprs = application_args(app);
        let args: Vec<IrExpr> = arg_exprs.iter().map(|a| self.lower_expr(a)).collect();
        if let Some(Expr::Name(callee)) = app.function()
            && is_constructor(callee.text())
        {
            return IrExpr::Constructor(IrConstructor {
                name: String::from(callee.text()),
                type_name: head_type_name(&result_type)
                    .unwrap_or_else(|| String::from(callee.text())),
                args,
                result_type,
            });
        }
        let func = app.function().expect("application has a callee");
        IrExpr::App(IrApp {
            func: Box::new(self.lower_expr(&func)),
            args,
            result_type,
        })
    }

    /// `receiver.field`, or a qualified name (`Mod.member`). The checker types
    /// the field node but never types a qualifier receiver as a value, so an
    /// untyped bare-name receiver marks the qualified-name case.
    fn lower_field(&self, field: &FieldExpr) -> IrExpr {
        let ty = self.node_type(field.syntax());
        let receiver = field.receiver().expect("field access has a receiver");
        let field_name = field.field().expect("field access names a field");
        if let Expr::Name(qualifier) = &receiver
            && self
                .checked
                .type_at(NodeKey::of_token(qualifier.syntax()))
                .is_none()
        {
            return IrExpr::Var(IrVar {
                name: format!("{}.{field_name}", qualifier.text()),
                ty,
            });
        }
        IrExpr::Field(IrField {
            receiver: Box::new(self.lower_expr(&receiver)),
            field: String::from(field_name),
            ty,
        })
    }

    /// `{ label: value, … }`.
    fn lower_record(&self, record: &RecordLit) -> IrExpr {
        let fields = record
            .fields()
            .filter_map(|f| {
                let label = f.name()?;
                let value = f.value()?;
                Some(IrRecordField {
                    label: String::from(label),
                    value: self.lower_expr(&value),
                })
            })
            .collect();
        IrExpr::Record(IrRecord {
            fields,
            ty: self.node_type(record.syntax()),
        })
    }

    /// `(a, b, …)`, including unit (`()`).
    fn lower_tuple(&self, tuple: &TupleLit) -> IrExpr {
        IrExpr::Tuple(IrTuple {
            elems: tuple.elements().map(|e| self.lower_expr(&e)).collect(),
            ty: self.node_type(tuple.syntax()),
        })
    }

    /// The unit value, used as a fallback for the rare malformed-node case.
    fn unit(&self) -> IrExpr {
        IrExpr::Tuple(IrTuple {
            elems: Vec::new(),
            ty: Type::tuple(Vec::new()),
        })
    }

    // ── patterns ─────────────────────────────────────────────────

    /// Lowers a pattern, carrying the type of the value it matches.
    fn lower_pattern(&self, pattern: &Pattern) -> IrPattern {
        let ty = self.node_type(pattern.syntax());
        match pattern {
            Pattern::Wildcard(_) => IrPattern::Wildcard(IrWildcardPat { ty }),
            Pattern::Bind(bind) => IrPattern::Bind(IrBindPat {
                name: String::from(bind.name().unwrap_or("")),
                ty,
            }),
            Pattern::Literal(lit) => IrPattern::Literal(IrLiteralPat {
                value: lit
                    .literal()
                    .map(|l| literal_value(&l))
                    .unwrap_or(LiteralValue::Int(Box::from("0"))),
                ty,
            }),
            Pattern::Tuple(tuple) => IrPattern::Tuple(IrTuplePat {
                elems: tuple.elements().map(|p| self.lower_pattern(&p)).collect(),
                ty,
            }),
            Pattern::Constructor(ctor) => IrPattern::Constructor(IrConstructorPat {
                name: String::from(ctor.name().unwrap_or("")),
                type_name: head_type_name(&ty).unwrap_or_default(),
                fields: ctor.fields().map(|p| self.lower_pattern(&p)).collect(),
                ty,
            }),
        }
    }

    // ── spans ────────────────────────────────────────────────────

    /// The source position of a declaration node: the 1-based line its first
    /// non-trivia token starts on (leading whitespace and comments are part of
    /// the node's range, so the node start itself can point at blank lines).
    fn span(&self, node: &hird_ast::SyntaxNode) -> IrSpan {
        let offset: u32 = node
            .children_with_tokens()
            .filter_map(|elem| elem.into_token())
            .find(|token| {
                !matches!(
                    token.kind(),
                    SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
                )
            })
            .map_or_else(
                || node.text_range().start(),
                |token| token.text_range().start(),
            )
            .into();
        let before = self.newlines.partition_point(|&nl| nl < offset);
        IrSpan {
            line: u32::try_from(before).unwrap_or(u32::MAX).saturating_add(1),
        }
    }

    // ── type lookup ──────────────────────────────────────────────

    /// The resolved type the checker recorded for `expr`.
    fn expr_type(&self, expr: &Expr) -> Type {
        self.checked
            .type_at(NodeKey::of_expr(expr))
            .cloned()
            .expect("every checked expression has a recorded type")
    }

    /// The resolved type the checker recorded for a CST node.
    fn node_type(&self, node: &hird_ast::SyntaxNode) -> Type {
        self.checked
            .type_at(NodeKey::of_node(node))
            .cloned()
            .expect("every checked node has a recorded type")
    }
}

// ── free helpers ─────────────────────────────────────────────────

/// The byte offsets of every newline in the file, ascending.
fn newline_offsets(file: &SourceFile) -> Vec<u32> {
    let mut newlines = Vec::new();
    let mut offset: u32 = 0;
    file.syntax().text().for_each_chunk(|chunk| {
        for (i, byte) in chunk.bytes().enumerate() {
            if byte == b'\n' {
                newlines.push(offset.saturating_add(u32::try_from(i).unwrap_or(u32::MAX)));
            }
        }
        offset = offset.saturating_add(u32::try_from(chunk.len()).unwrap_or(u32::MAX));
    });
    newlines
}

/// The actor body field named `member`, if present.
fn actor_field(decl: &ActorDecl, member: &str) -> Option<ActorField> {
    decl.fields().find(|f| f.name() == Some(member))
}

/// The supervisor body field named `field`, if present.
fn supervisor_field(decl: &SupervisorDecl, field: &str) -> Option<SupervisorField> {
    decl.fields().find(|f| f.name() == Some(field))
}

/// The identifier value of a supervisor field (`strategy: one_for_one`).
fn supervisor_ident(decl: &SupervisorDecl, field: &str) -> Option<String> {
    match supervisor_field(decl, field)?.value()? {
        Expr::Name(name) => Some(String::from(name.text())),
        _ => None,
    }
}

/// The integer value of a supervisor field (`intensity: 5`, `period: 60`).
fn supervisor_int(decl: &SupervisorDecl, field: &str) -> Option<u32> {
    match supervisor_field(decl, field)?.value()? {
        Expr::Literal(lit) if lit.kind() == SyntaxKind::INT => lit.text().parse().ok(),
        _ => None,
    }
}

/// The record field named `field`, if present.
fn record_field(spec: &RecordLit, field: &str) -> Option<RecordField> {
    spec.fields().find(|f| f.name() == Some(field))
}

/// The identifier value of a child-spec field (`id`, `actor`, `restart`).
fn record_ident(spec: &RecordLit, field: &str) -> Option<String> {
    match record_field(spec, field)?.value()? {
        Expr::Name(name) => Some(String::from(name.text())),
        _ => None,
    }
}

/// The argument expressions of an application. A tuple-literal argument is the
/// argument list (`f(a, b)` is two arguments, `f()` zero); anything else is a
/// single argument.
fn application_args(app: &AppExpr) -> Vec<Expr> {
    match app.argument() {
        Some(Expr::Tuple(tuple)) => tuple.elements().collect(),
        Some(other) => Vec::from([other]),
        None => Vec::new(),
    }
}

/// A literal's value, tagged by its token kind and carrying its source text.
fn literal_value(lit: &Literal) -> LiteralValue {
    let text = Box::from(lit.text());
    match lit.kind() {
        SyntaxKind::INT => LiteralValue::Int(text),
        SyntaxKind::FLOAT => LiteralValue::Float(text),
        // The checker accepts only INT, FLOAT, and STRING literals.
        _ => LiteralValue::Str(text),
    }
}

/// A synthetic nullary `Bool` constructor pattern (`True`/`False`), for the
/// `if`-to-`match` desugaring.
fn bool_pattern(name: &str) -> IrPattern {
    IrPattern::Constructor(IrConstructorPat {
        name: String::from(name),
        type_name: String::from("Bool"),
        fields: Vec::new(),
        ty: Type::bool(),
    })
}

/// Whether a name is a constructor: the naming convention reserves
/// `PascalCase` for constructors and `snake_case` for values.
fn is_constructor(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

/// The head constructor name of a type (`List<Int>` → `List`), or `None` when
/// the type is not a constructor application.
fn head_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::TyCon(name, _) => Some(String::from(name.as_str())),
        _ => None,
    }
}

/// The canonical name of a binary operator. Logical operators normalise to
/// their Unicode form regardless of how they were written; the rest are ASCII
/// already.
fn canonical_operator(op: &str) -> String {
    let canonical = match op {
        "&&" | "∧" => "∧",
        "||" | "∨" => "∨",
        other => other,
    };
    String::from(canonical)
}

/// The field types of a constructor, read from its generalised `scheme` and
/// renamed so type-parameter variables render with their declared names
/// (`params`).
///
/// A constructor's scheme is `∀…. (fields) → Owner<v₁ … vₙ>`, where the result
/// arguments `v₁ … vₙ` are exactly the owner's parameters in declaration
/// order. Mapping each `vᵢ` to the declared name `paramsᵢ` makes the field
/// types read back as written (`a`, `List<a>`).
fn constructor_field_types(scheme: &Type, params: &[String]) -> Vec<Type> {
    let inner = match scheme {
        Type::TyForall(_, _, body) => body.as_ref(),
        other => other,
    };
    let (fields, result) = match inner {
        Type::TyFn(fields, result, _) => (fields.as_slice(), result.as_ref()),
        // A nullary constructor: no fields, the type itself is the result.
        _ => (&[][..], inner),
    };
    let rename = parameter_rename(result, params);
    fields
        .iter()
        .map(|f| f.substitute(&rename, &BTreeMap::new()))
        .collect()
}

/// Builds the variable-to-name map for [`constructor_field_types`] from a
/// constructor's result type `Owner<v₁ … vₙ>` and the owner's declared
/// parameter names.
fn parameter_rename(result: &Type, params: &[String]) -> BTreeMap<u32, Type> {
    let mut map = BTreeMap::new();
    if let Type::TyCon(_, args) = result {
        for (arg, name) in args.iter().zip(params) {
            if let Type::TyVar(id) = arg {
                map.insert(*id, Type::con(name.as_str(), Vec::new()));
            }
        }
    }
    map
}

/// The args type, result type, and trailing row of a tool's generalised
/// function scheme `∀…. (args) → result ! ({Tool<name>} ∪ trailing)`, with
/// quantified variables renamed to the declared parameter names (`params`)
/// and the implicit `Tool<name>` effect removed from the row.
fn tool_signature(scheme: &Type, params: &[String], name: &str) -> Option<(Type, Type, EffectRow)> {
    let body = match scheme {
        Type::TyForall(_, _, body) => body.as_ref(),
        other => other,
    };
    let Type::TyFn(inputs, output, row) = body else {
        return None;
    };
    let input = inputs.first()?;
    let rename = tool_parameter_rename(scheme, params);
    let rows = BTreeMap::new();
    let mut trailing = EffectRow::empty();
    for effect in row.effects() {
        if is_tool_marker_effect(effect, name) {
            continue;
        }
        trailing.insert(effect.map_args(|a| a.substitute(&rename, &rows)));
    }
    Some((
        input.substitute(&rename, &rows),
        output.substitute(&rename, &rows),
        trailing,
    ))
}

/// Builds the variable-to-name map for [`tool_signature`]. A tool signature
/// elaborates one fresh variable per declared parameter, in declaration
/// order, so the quantified variables in ascending id order mirror the
/// declared names.
fn tool_parameter_rename(scheme: &Type, params: &[String]) -> BTreeMap<u32, Type> {
    let Type::TyForall(tvars, _, _) = scheme else {
        return BTreeMap::new();
    };
    let mut ids = tvars.clone();
    ids.sort_unstable();
    ids.iter()
        .zip(params)
        .map(|(id, name)| (*id, Type::con(name.as_str(), Vec::new())))
        .collect()
}

/// Whether `effect` is a given tool's implicit effect (`Tool<name>`).
fn is_tool_marker_effect(effect: &Effect, name: &str) -> bool {
    effect.head().as_str() == "Tool"
        && matches!(effect.args(),
            [Type::TyCon(marker, args)] if marker.as_str() == name && args.is_empty())
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Expression inference and pattern checking.
//!
//! Application follows the syntactic argument shape: a tuple-literal
//! argument is an n-ary argument list (`f(a, b)` is a 2-ary call, `f()` a
//! 0-ary one), anything else is a single argument, and a tuple *value* is
//! passed as `f((a, b))`. Operators are monomorphic (`Int` arithmetic and
//! ordering, polymorphic equality, `Bool` connectives). A `handle` block types
//! as its body; its effect row is the body's effects minus the handled effects
//! plus the handlers' own effects.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::mem;

use hird_ast::{
    AppExpr, AstNode, BinOpExpr, Expr, FieldExpr, HandleBlock, IfExpr, LambdaExpr, LetExpr,
    MatchExpr, Pattern, RecordLit, ReplyExpr, RequestExpr, SendExpr, SpawnExpr,
};
use hird_lex::Span;
use hird_parse::SyntaxKind;
use hird_types::{Effect, EffectRow, Label, Name, Type, handle_row, unify};

use crate::checker::{Aborted, Checked, Checker};
use crate::diag::CheckCode;
use crate::elaborate::Scope;
use crate::registry::CtorInfo;
use crate::{
    ModuleName, NodeKey, expr_span, name_token_span, node_span, token_span, type_expr_span,
};

impl Checker {
    /// Infers the type of `expr`, recording it in the node table.
    pub(crate) fn infer_expr(&mut self, expr: &Expr) -> Checked<Type> {
        let ty = self.infer_expr_inner(expr)?;
        self.types.push((NodeKey::of_expr(expr), ty.clone()));
        Ok(ty)
    }

    /// Dispatches over the expression forms.
    fn infer_expr_inner(&mut self, expr: &Expr) -> Checked<Type> {
        match expr {
            Expr::Literal(lit) => match lit.kind() {
                SyntaxKind::INT => Ok(Type::int()),
                SyntaxKind::FLOAT => Ok(Type::float()),
                SyntaxKind::STRING => Ok(Type::string()),
                _ => Err(Aborted),
            },
            Expr::Name(name) => {
                let text = name.text();
                let Some(scheme) = self.env.lookup(text).cloned() else {
                    let span = token_span(name.syntax(), self.source_id);
                    // A foreign opaque constructor is in the registry but never
                    // bound as a value, so construction outside its module lands
                    // here; name the type rather than report an unbound name.
                    if let Some(aborted) = self.opaque_construct_error(text, span) {
                        return Err(aborted);
                    }
                    // An actor name is not a value: its state and members are
                    // encapsulated, reachable only within its handlers.
                    if self.actors.contains_key(text) {
                        return Err(self.error(
                            CheckCode::C0040,
                            span,
                            format!(
                                "`{text}` is an actor, not a value; its state is \
                                 only accessible within its own handlers"
                            ),
                        ));
                    }
                    return Err(self.error(
                        CheckCode::C0003,
                        span,
                        format!("unbound name `{text}`"),
                    ));
                };
                // A constructor carrying a reply channel has exactly one wire
                // shape: it is applicable only as `request`'s builder, which
                // resolves it without reaching this value path.
                if self.registry.ctor_carries_reply_to(text) {
                    let span = token_span(name.syntax(), self.source_id);
                    return Err(self.error(
                        CheckCode::C0043,
                        span,
                        format!(
                            "`{text}` carries a reply channel; it can only be used as \
                             the message builder of `request`"
                        ),
                    ));
                }
                Ok(self.subst.instantiate(&scheme))
            }
            Expr::Let(le) => self.infer_let(le),
            Expr::Lambda(lambda) => self.infer_lambda(lambda),
            Expr::If(ife) => self.infer_if(ife),
            Expr::Match(me) => self.infer_match(me),
            Expr::Handle(handle) => self.infer_handle(handle),
            Expr::Spawn(spawn) => self.infer_spawn(spawn),
            Expr::Send(send) => self.infer_send(send),
            Expr::Request(request) => self.infer_request(request),
            Expr::Reply(reply) => self.infer_reply(reply),
            Expr::BinOp(op) => self.infer_binop(op),
            Expr::App(app) => self.infer_app(app),
            Expr::Field(field) => self.infer_field(field),
            Expr::Record(record) => self.infer_record(record),
            Expr::Tuple(tuple) => {
                let mut elems = Vec::new();
                for elem in tuple.elements() {
                    elems.push(self.infer_expr(&elem)?);
                }
                Ok(Type::tuple(elems))
            }
            Expr::List(list) => {
                let elem_ty = self.subst.fresh_type();
                for elem in list.elements() {
                    let span = expr_span(&elem, self.source_id);
                    let ty = self.infer_expr(&elem)?;
                    self.unify_at(&elem_ty, &ty, span)?;
                }
                Ok(Type::list(elem_ty))
            }
            Expr::Paren(paren) => {
                let Some(inner) = paren.inner() else {
                    return Err(Aborted);
                };
                self.infer_expr(&inner)
            }
        }
    }

    /// `let name [: T] = value in body` — generalise the value, bind the
    /// scheme, infer the body.
    fn infer_let(&mut self, le: &LetExpr) -> Checked<Type> {
        let Some(name) = le.name() else {
            return Err(Aborted);
        };
        let name = String::from(name);
        let Some(value) = le.value() else {
            return Err(Aborted);
        };

        self.subst.enter_level();
        let mut value_ty = self.infer_expr(&value);
        if let Ok(ty) = &value_ty
            && let Some(annotation) = le.annotation()
        {
            let mut scope = Scope::new();
            let span = expr_span(&value, self.source_id);
            value_ty = match self.elaborate_fresh(&annotation, &mut scope) {
                Ok(ann_ty) => self.unify_at(&ann_ty, ty, span).map(|()| ann_ty),
                Err(aborted) => Err(aborted),
            };
        }
        self.subst.exit_level();
        let value_ty = value_ty?;
        let scheme = self.subst.generalize(&value_ty);

        self.env.push_scope();
        let span = name_token_span(le.syntax(), self.source_id);
        self.bind_value(&name, scheme, span);
        let body_ty = match le.body() {
            Some(body) => self.infer_expr(&body),
            None => Err(Aborted),
        };
        self.env.pop_scope();
        body_ty
    }

    /// Infers with a fresh, empty effect accumulator, returning the result and
    /// the effects it performed. The enclosing row and provenance are saved
    /// across the call and restored after, so a lambda's or `handle` body's
    /// effects stay out of the row they sit in instead of folding into it.
    fn infer_in_fresh_row(
        &mut self,
        infer: impl FnOnce(&mut Self) -> Checked<Type>,
    ) -> (Checked<Type>, EffectRow) {
        let saved_row = mem::take(&mut self.current_row);
        let saved_prov = mem::take(&mut self.current_prov);
        let res = infer(self);
        let row = mem::replace(&mut self.current_row, saved_row);
        self.current_prov = saved_prov;
        (res, row)
    }

    /// `λx y → body` — monomorphic parameters, n-ary function type.
    fn infer_lambda(&mut self, lambda: &LambdaExpr) -> Checked<Type> {
        let mut params: Vec<(String, Span, NodeKey)> = Vec::new();
        for element in lambda.syntax().children_with_tokens() {
            let Some(token) = element.into_token() else {
                continue;
            };
            if token.kind() == SyntaxKind::ARROW {
                break;
            }
            if token.kind() == SyntaxKind::IDENT {
                params.push((
                    String::from(token.text()),
                    token_span(token, self.source_id),
                    NodeKey::of_token(token),
                ));
            }
        }

        let mut param_tys = Vec::new();
        self.env.push_scope();
        for (name, span, key) in &params {
            let ty = self.subst.fresh_type();
            self.types.push((*key, ty.clone()));
            self.bind_value(name, ty.clone(), *span);
            param_tys.push(ty);
        }
        // A lambda is pure as an expression: its body's effects belong to its
        // function type, not the enclosing row. Infer the body into a fresh
        // accumulator so they stay apart; the lambda's provenance is discarded,
        // as the enclosing function never consults it.
        let (body_res, body_row) = self.infer_in_fresh_row(|c| match lambda.body() {
            Some(body) => c.infer_expr(&body),
            None => Err(Aborted),
        });
        self.env.pop_scope();
        Ok(Type::func_eff(param_tys, body_res?, body_row))
    }

    /// `handle { Effect → handler, … } in body` — DI-style effect handlers.
    ///
    /// The block's value type is the body's type. Each arm must name a declared
    /// effect at the correct arity and bind it to a function; a `Tool<Marker>`
    /// arm's handler is further checked against the tool's operation signature
    /// ([`Checker::check_tool_handler`]), and a non-tool marker is an error.
    /// Non-tool effects keep the structural check — unknown effect, wrong
    /// arity, and a non-function handler are the reported shapes. The block's
    /// effect row is the body's effects minus the handled effects plus the
    /// handlers' own effects.
    fn infer_handle(&mut self, handle: &HandleBlock) -> Checked<Type> {
        let Some(body) = handle.body() else {
            return Err(Aborted);
        };
        let handle_span = node_span(handle.syntax(), self.source_id);

        // The body's effects belong to the handle, not yet the enclosing row:
        // infer them into a fresh accumulator the way a lambda body's are, so
        // the handled effects can be subtracted before the result rejoins the
        // enclosing row.
        let (body_res, body_row) = self.infer_in_fresh_row(|c| c.infer_expr(&body));
        let body_ty = body_res?;

        // Each arm names a declared effect (validated for arity) and binds it to
        // a function whose own effects join the block's row. Evaluating a
        // handler expression performs its effects at the handle site, so those
        // accrue to the enclosing row as any expression's do.
        let mut handled = EffectRow::empty();
        let mut handler_effects = EffectRow::empty();
        for arm in handle.arms() {
            // A `Tool<Marker>` head makes this a tool arm: the handled tool,
            // whose operation signature the handler is checked against below.
            let mut tool: Option<Name> = None;
            if let Some(effect_expr) = arm.effect() {
                let mut scope = Scope::new();
                if let Ok(effect) = self.elaborate_handle_effect(&effect_expr, &mut scope) {
                    self.handled_effects
                        .push((NodeKey::of_node(arm.syntax()), effect.clone()));
                    if effect.head().as_str() == "Tool"
                        && let [marker] = effect.args()
                    {
                        match self.subst.resolve(marker) {
                            Type::TyCon(name, _) if self.tool_signatures.contains_key(&name) => {
                                tool = Some(name);
                            }
                            other => {
                                let span = type_expr_span(&effect_expr, self.source_id);
                                let _ = self.error(
                                    CheckCode::C0033,
                                    span,
                                    format!("`{other}` is not a declared tool"),
                                );
                            }
                        }
                    }
                    handled.insert(effect);
                }
            }
            if let Some(handler) = arm.handler() {
                let handler_ty = self.infer_expr(&handler)?;
                let span = expr_span(&handler, self.source_id);
                match self.subst.resolve(&handler_ty) {
                    Type::TyFn(_, _, row) => {
                        // `resolve` already resolved the function type's row, so
                        // its effects are canonical; a later arm's solving is
                        // caught by the final resolve of `handler_effects`.
                        for effect in row.effects() {
                            handler_effects.insert(effect.clone());
                        }
                        if let Some(tool) = tool {
                            self.check_tool_handler(&tool, &handler_ty, span);
                        }
                    }
                    other => {
                        let _ = self.error(
                            CheckCode::C0031,
                            span,
                            format!(
                                "a handle arm's handler must be a function, but this has type `{other}`"
                            ),
                        );
                    }
                }
            }
        }

        // (body − handled) ∪ handler effects, recorded for the IR and rejoined
        // to the enclosing row at the handle's span.
        let body_row = self.subst.resolve_row(&body_row);
        let handled = self.subst.resolve_row(&handled);
        let handler_effects = self.subst.resolve_row(&handler_effects);
        let net = handle_row(&body_row, &handled, &handler_effects);
        self.effect_rows
            .push((NodeKey::of_node(handle.syntax()), net.clone()));
        self.add_effects(&net, handle_span);
        Ok(body_ty)
    }

    /// Checks a tool arm's handler against the handled tool's operation
    /// signature: the signature is instantiated with fresh type variables and
    /// unified with the handler's type, so a monomorphic handler for a generic
    /// tool is accepted. The expected row is a fresh open row variable — a
    /// mock may be pure and need not carry the tool's declared trailing row.
    /// A mismatch is reported as C0034, not a raw unification error.
    fn check_tool_handler(&mut self, tool: &Name, handler_ty: &Type, span: Span) {
        let Some(scheme) = self.tool_signatures.get(tool).cloned() else {
            return;
        };
        let Type::TyFn(params, ret, _) = self.subst.instantiate(&scheme) else {
            return;
        };
        // Rendered before unifying: a failed unification may leave partial
        // bindings that would distort the displayed types.
        let expected_disp = Type::func(params.clone(), (*ret).clone()).normalized();
        let handler_disp = self.subst.resolve(handler_ty).normalized();
        let expected = Type::TyFn(params, ret, EffectRow::of_var(self.subst.fresh_row()));
        if unify(&mut self.subst, &expected, handler_ty, span).is_err() {
            let _ = self.error(
                CheckCode::C0034,
                span,
                format!(
                    "handler for tool `{tool}` must have type `{expected_disp}`, \
                     but this has type `{handler_disp}`"
                ),
            );
        }
    }

    /// `spawn(Actor, args…)` — starts an actor, returning a typed reference.
    ///
    /// The actor name resolves in the actor namespace; the arguments are
    /// checked against the actor's init parameters. The expression's type is
    /// `Pid<Msg>` and its effect is `Spawn<Msg>`, where `Msg` is the actor's
    /// message type. Init's own effects are not the spawner's: they run in
    /// the spawned process (per-process effect semantics).
    fn infer_spawn(&mut self, spawn: &SpawnExpr) -> Checked<Type> {
        let span = node_span(spawn.syntax(), self.source_id);
        let Some(name) = spawn.actor_name() else {
            return Err(Aborted);
        };
        let Some(info) = self.actors.get(name) else {
            let at = spawn
                .actor_token()
                .map_or(span, |t| token_span(t, self.source_id));
            return Err(self.error(
                CheckCode::C0039,
                at,
                format!("`{name}` is not a declared actor"),
            ));
        };
        let params = info.init_params.clone();
        let message = Type::con(info.message.as_str(), Vec::new());
        let args: Vec<Expr> = spawn.args().collect();
        if args.len() != params.len() {
            return Err(self.error(
                CheckCode::C0039,
                span,
                format!(
                    "this spawn supplies {} argument(s), but actor `{name}`'s \
                     init takes {}",
                    args.len(),
                    params.len()
                ),
            ));
        }
        for (param, arg) in params.iter().zip(&args) {
            let arg_span = expr_span(arg, self.source_id);
            let arg_ty = self.infer_expr(arg)?;
            self.unify_at(param, &arg_ty, arg_span)?;
        }
        let row = EffectRow::closed([Effect::parametric("Spawn", Vec::from([message.clone()]))]);
        self.add_effects(&row, span);
        Ok(Type::con("Pid", Vec::from([message])))
    }

    /// `send(pid, msg)` — fire-and-forget delivery to a typed reference.
    ///
    /// The destination must be a `Pid<Msg>` and the message a `Msg`; the
    /// expression is unit with a `Send<Msg>` effect. Effects are per-process
    /// and local: the sender's row records the send, never what the receiver
    /// goes on to do.
    fn infer_send(&mut self, send: &SendExpr) -> Checked<Type> {
        let span = node_span(send.syntax(), self.source_id);
        let (Some(pid), Some(message)) = (send.pid(), send.message()) else {
            return Err(Aborted);
        };
        let msg_ty = self.check_pid(&pid)?;
        let message_span = expr_span(&message, self.source_id);
        let message_ty = self.infer_expr(&message)?;
        self.unify_at(&msg_ty, &message_ty, message_span)?;
        let row = EffectRow::closed([Effect::parametric("Send", Vec::from([msg_ty]))]);
        self.add_effects(&row, span);
        Ok(Type::tuple(Vec::new()))
    }

    /// `request(pid, ctor)` — send with an embedded reply channel, then await.
    ///
    /// The second argument builds the message around a fresh reply channel:
    /// it must be a `ReplyTo<T> → Msg` function (typically a message
    /// constructor). The expression's type is the reply type `T`, and its
    /// effects are `Send<Msg>` for the send plus `Await<T>` for the blocking
    /// wait — two distinct effects, never a combined head. The wait has a
    /// fixed timeout whose expiry exits the caller, so no `Exn` joins the row.
    fn infer_request(&mut self, request: &RequestExpr) -> Checked<Type> {
        let span = node_span(request.syntax(), self.source_id);
        let (Some(pid), Some(message_fn)) = (request.pid(), request.message_fn()) else {
            return Err(Aborted);
        };
        let msg_ty = self.check_pid(&pid)?;
        let reply_ty = self.subst.fresh_type();
        let fn_span = expr_span(&message_fn, self.source_id);
        let fn_ty = self.request_builder_type(&message_fn, fn_span)?;
        // A fresh row for the builder: a constructor is pure, but whatever the
        // builder performs happens here, in the caller, so its row joins the
        // caller's alongside the messaging effects.
        let builder_row = EffectRow::of_var(self.subst.fresh_row());
        let expected = Type::func_eff(
            Vec::from([Type::con("ReplyTo", Vec::from([reply_ty.clone()]))]),
            msg_ty.clone(),
            builder_row.clone(),
        );
        self.unify_at(&expected, &fn_ty, fn_span)?;
        self.add_effects(&builder_row, span);
        let row = EffectRow::closed([
            Effect::parametric("Send", Vec::from([msg_ty])),
            Effect::parametric("Await", Vec::from([reply_ty.clone()])),
        ]);
        self.add_effects(&row, span);
        Ok(reply_ty)
    }

    /// Resolves a `request` builder, which must be a bare message constructor:
    /// only then can codegen statically strip the reply channel to build the
    /// call payload. Resolving it here — rather than through
    /// [`Checker::infer_expr`] — is what lets its one legal use escape the
    /// call-constructor ban ([`CheckCode::C0043`]). A lambda or any other
    /// expression is rejected ([`CheckCode::C0042`]); whether the constructor
    /// actually carries a reply channel is left to the caller's unification.
    fn request_builder_type(&mut self, message_fn: &Expr, span: Span) -> Checked<Type> {
        let mut expr = message_fn.clone();
        while let Expr::Paren(paren) = &expr {
            let Some(inner) = paren.inner() else {
                return Err(Aborted);
            };
            expr = inner;
        }
        if let Expr::Name(name) = &expr {
            let scheme = self
                .registry
                .ctor(name.text())
                .map(|info| info.scheme.clone());
            if let Some(scheme) = scheme {
                let fn_ty = self.subst.instantiate(&scheme);
                self.types.push((NodeKey::of_expr(&expr), fn_ty.clone()));
                return Ok(fn_ty);
            }
        }
        Err(self.error(
            CheckCode::C0042,
            span,
            String::from(
                "the message builder of `request` must be a bare message constructor \
                 (e.g. `Get`), not an arbitrary function",
            ),
        ))
    }

    /// `reply(reply_to, value)` — answers a request on its typed channel.
    ///
    /// The only operation on `ReplyTo<T>`: the value must be a `T`, the
    /// expression is unit, and the effect is plain `Send<T>` — no dedicated
    /// effect head.
    fn infer_reply(&mut self, reply: &ReplyExpr) -> Checked<Type> {
        let span = node_span(reply.syntax(), self.source_id);
        let (Some(reply_to), Some(value)) = (reply.reply_to(), reply.value()) else {
            return Err(Aborted);
        };
        let val_ty = self.subst.fresh_type();
        let reply_to_span = expr_span(&reply_to, self.source_id);
        let reply_to_ty = self.infer_expr(&reply_to)?;
        self.unify_at(
            &Type::con("ReplyTo", Vec::from([val_ty.clone()])),
            &reply_to_ty,
            reply_to_span,
        )?;
        let value_span = expr_span(&value, self.source_id);
        let value_ty = self.infer_expr(&value)?;
        self.unify_at(&val_ty, &value_ty, value_span)?;
        let row = EffectRow::closed([Effect::parametric("Send", Vec::from([val_ty]))]);
        self.add_effects(&row, span);
        Ok(Type::tuple(Vec::new()))
    }

    /// Infers a messaging destination and pins it to `Pid<Msg>`, returning the
    /// message type `Msg`.
    fn check_pid(&mut self, pid: &Expr) -> Checked<Type> {
        let msg_ty = self.subst.fresh_type();
        let span = expr_span(pid, self.source_id);
        let pid_ty = self.infer_expr(pid)?;
        self.unify_at(
            &Type::con("Pid", Vec::from([msg_ty.clone()])),
            &pid_ty,
            span,
        )?;
        Ok(msg_ty)
    }

    /// `if c then a else b` — `Bool` condition, unified branches.
    fn infer_if(&mut self, ife: &IfExpr) -> Checked<Type> {
        let Some(cond) = ife.condition() else {
            return Err(Aborted);
        };
        let cond_span = expr_span(&cond, self.source_id);
        let cond_ty = self.infer_expr(&cond)?;
        self.unify_at(&Type::bool(), &cond_ty, cond_span)?;

        let Some(then_branch) = ife.then_branch() else {
            return Err(Aborted);
        };
        let then_ty = self.infer_expr(&then_branch)?;
        let Some(else_branch) = ife.else_branch() else {
            return Err(Aborted);
        };
        let else_span = expr_span(&else_branch, self.source_id);
        let else_ty = self.infer_expr(&else_branch)?;
        self.unify_at(&then_ty, &else_ty, else_span)?;
        Ok(then_ty)
    }

    /// `match scrutinee { arms }` — patterns check against the scrutinee,
    /// arm bodies unify together. Exhaustiveness is a separate pass.
    fn infer_match(&mut self, me: &MatchExpr) -> Checked<Type> {
        let Some(scrutinee) = me.scrutinee() else {
            return Err(Aborted);
        };
        let scrutinee_ty = self.infer_expr(&scrutinee)?;
        let mut result: Option<Type> = None;
        for arm in me.arms() {
            let Some(pattern) = arm.pattern() else {
                return Err(Aborted);
            };
            self.env.push_scope();
            let pattern_res = self.check_pattern(&pattern, &scrutinee_ty);
            let body_res = match (&pattern_res, arm.body()) {
                (Ok(()), Some(body)) => self
                    .infer_expr(&body)
                    .map(|ty| (ty, expr_span(&body, self.source_id))),
                _ => Err(Aborted),
            };
            self.env.pop_scope();
            pattern_res?;
            let (body_ty, body_span) = body_res?;
            match &result {
                None => result = Some(body_ty),
                Some(first) => self.unify_at(first, &body_ty, body_span)?,
            }
        }
        // Every arm checked cleanly: now decide coverage over the resolved
        // scrutinee type, which the constructor patterns have pinned down.
        let scrutinee_ty = self.subst.resolve(&scrutinee_ty);
        self.check_match(me, &scrutinee_ty);
        Ok(result.unwrap_or_else(|| self.subst.fresh_type()))
    }

    /// Checks `pattern` against the `expected` scrutinee type, binding
    /// pattern variables into the current scope.
    pub(crate) fn check_pattern(&mut self, pattern: &Pattern, expected: &Type) -> Checked<()> {
        self.types
            .push((NodeKey::of_node(pattern.syntax()), expected.clone()));
        let span = node_span(pattern.syntax(), self.source_id);
        match pattern {
            Pattern::Wildcard(_) => Ok(()),
            Pattern::Bind(bind) => {
                let Some(name) = bind.name() else {
                    return Err(Aborted);
                };
                self.bind_value(name, expected.clone(), span);
                Ok(())
            }
            Pattern::Literal(lit) => {
                let Some(literal) = lit.literal() else {
                    return Err(Aborted);
                };
                let ty = match literal.kind() {
                    SyntaxKind::INT => Type::int(),
                    SyntaxKind::FLOAT => Type::float(),
                    SyntaxKind::STRING => Type::string(),
                    _ => return Err(Aborted),
                };
                self.unify_at(expected, &ty, span)
            }
            Pattern::Tuple(tuple) => {
                let elements: Vec<Pattern> = tuple.elements().collect();
                let elem_tys: Vec<Type> =
                    elements.iter().map(|_| self.subst.fresh_type()).collect();
                self.unify_at(expected, &Type::tuple(elem_tys.clone()), span)?;
                for (element, ty) in elements.iter().zip(&elem_tys) {
                    self.check_pattern(element, ty)?;
                }
                Ok(())
            }
            Pattern::Constructor(ctor) => {
                let Some(name) = ctor.name() else {
                    return Err(Aborted);
                };
                // Snapshot what the gate needs, ending the registry borrow
                // before any diagnostic is pushed.
                let lookup = self.registry.ctor(name).map(|info| {
                    (
                        info.scheme.clone(),
                        opaque_violation(info, self.current_module.as_ref(), "destructure"),
                    )
                });
                let Some((scheme, violation)) = lookup else {
                    return Err(self.error(
                        CheckCode::C0007,
                        span,
                        format!("unknown constructor `{name}`"),
                    ));
                };
                if let Some(message) = violation {
                    return Err(self.error(CheckCode::C0021, span, message));
                }
                let instance = self.subst.instantiate(&scheme);
                let (fields, result_ty) = match instance {
                    Type::TyFn(params, ret, _) => (params, *ret),
                    other => (Vec::new(), other),
                };
                let sub_patterns: Vec<Pattern> = ctor.fields().collect();
                if sub_patterns.len() != fields.len() {
                    return Err(self.error(
                        CheckCode::C0008,
                        span,
                        format!(
                            "`{name}` has {} field(s), but the pattern names {}",
                            fields.len(),
                            sub_patterns.len()
                        ),
                    ));
                }
                self.unify_at(expected, &result_ty, span)?;
                for (sub, field_ty) in sub_patterns.iter().zip(&fields) {
                    self.check_pattern(sub, field_ty)?;
                }
                Ok(())
            }
        }
    }

    /// Binary operators per the v0.1 table: `Int` arithmetic and ordering,
    /// polymorphic equality, `Bool` connectives. No overloading.
    fn infer_binop(&mut self, op: &BinOpExpr) -> Checked<Type> {
        let Some(kind) = binop_kind(op) else {
            return Err(Aborted);
        };
        let Some(lhs) = op.lhs() else {
            return Err(Aborted);
        };
        let Some(rhs) = op.rhs() else {
            return Err(Aborted);
        };
        let lhs_span = expr_span(&lhs, self.source_id);
        let rhs_span = expr_span(&rhs, self.source_id);
        let lhs_ty = self.infer_expr(&lhs)?;
        let rhs_ty = self.infer_expr(&rhs)?;
        match kind {
            SyntaxKind::PLUS | SyntaxKind::MINUS | SyntaxKind::STAR | SyntaxKind::SLASH => {
                self.unify_at(&Type::int(), &lhs_ty, lhs_span)?;
                self.unify_at(&Type::int(), &rhs_ty, rhs_span)?;
                Ok(Type::int())
            }
            SyntaxKind::LT | SyntaxKind::LE | SyntaxKind::GT | SyntaxKind::GE => {
                self.unify_at(&Type::int(), &lhs_ty, lhs_span)?;
                self.unify_at(&Type::int(), &rhs_ty, rhs_span)?;
                Ok(Type::bool())
            }
            SyntaxKind::EQ_EQ | SyntaxKind::BANG_EQ => {
                self.unify_at(&lhs_ty, &rhs_ty, rhs_span)?;
                Ok(Type::bool())
            }
            SyntaxKind::ANDAND | SyntaxKind::OROR => {
                self.unify_at(&Type::bool(), &lhs_ty, lhs_span)?;
                self.unify_at(&Type::bool(), &rhs_ty, rhs_span)?;
                Ok(Type::bool())
            }
            _ => Err(Aborted),
        }
    }

    /// Application with syntactic argument shape (see module docs).
    fn infer_app(&mut self, app: &AppExpr) -> Checked<Type> {
        let Some(callee) = app.function() else {
            return Err(Aborted);
        };
        let callee_ty = self.infer_expr(&callee)?;
        // A tuple-literal argument is an argument list, not a tuple value;
        // it gets no entry in the node table because it is not an expression
        // in this position.
        let args: Vec<Expr> = match app.argument() {
            Some(Expr::Tuple(tuple)) => tuple.elements().collect(),
            Some(other) => Vec::from([other]),
            None => Vec::new(),
        };
        let mut arg_tys = Vec::new();
        for arg in &args {
            arg_tys.push(self.infer_expr(arg)?);
        }
        let app_span = node_span(app.syntax(), self.source_id);
        match self.subst.resolve(&callee_ty) {
            Type::TyFn(params, ret, row) => {
                if params.len() != arg_tys.len() {
                    return Err(self.error(
                        CheckCode::C0006,
                        app_span,
                        format!(
                            "this call supplies {} argument(s), but the function takes {}",
                            arg_tys.len(),
                            params.len()
                        ),
                    ));
                }
                for ((param, arg_ty), arg) in params.iter().zip(&arg_tys).zip(&args) {
                    let span = expr_span(arg, self.source_id);
                    self.unify_at(param, arg_ty, span)?;
                }
                // The callee's effects become the caller's, recorded at this call.
                self.add_effects(&row, app_span);
                Ok(*ret)
            }
            _ => {
                let ret = self.subst.fresh_type();
                // A not-yet-resolved callee gets a fresh effect-row variable, so
                // applying it stays effect-polymorphic (the row is generalised
                // for an interior function, or solved against the declared row
                // of a top-level one) rather than forced pure.
                let row = EffectRow::of_var(self.subst.fresh_row());
                let fn_ty = Type::func_eff(arg_tys, ret.clone(), row.clone());
                self.unify_at(&fn_ty, &callee_ty, app_span)?;
                self.add_effects(&row, app_span);
                Ok(ret)
            }
        }
    }

    /// Field access requires an already-determined record type; row
    /// polymorphism is a later phase.
    ///
    /// A bare name receiver that resolves in the module namespace makes this a
    /// qualified name (`Mod.member`) rather than field access; the `PascalCase`
    /// casing of module qualifiers keeps the two from ever overlapping with a
    /// value's field (`point.x`).
    fn infer_field(&mut self, field: &FieldExpr) -> Checked<Type> {
        let Some(receiver) = field.receiver() else {
            return Err(Aborted);
        };
        let Some(name) = field.field() else {
            return Err(Aborted);
        };
        let span = node_span(field.syntax(), self.source_id);
        if let Expr::Name(recv) = &receiver
            && let Some(member) = self
                .modules
                .get(recv.text())
                .map(|vals| vals.get(name).cloned())
        {
            // The receiver is a module qualifier: resolve against its exports
            // without ever typing the receiver as a value.
            return match member {
                Some(scheme) => Ok(self.subst.instantiate(&scheme)),
                None => Err(self.error(
                    CheckCode::C0024,
                    span,
                    format!("module `{}` has no exported value `{name}`", recv.text()),
                )),
            };
        }
        let receiver_ty = self.infer_expr(&receiver)?;
        match self.subst.resolve(&receiver_ty) {
            Type::TyRecord(fields) => match fields.get(&Label::new(name)) {
                Some(ty) => Ok(ty.clone()),
                None => {
                    let record = Type::TyRecord(fields.clone());
                    Err(self.error(
                        CheckCode::C0010,
                        span,
                        format!("record `{record}` has no field `{name}`"),
                    ))
                }
            },
            Type::TyVar(_) => Err(self.error(
                CheckCode::C0009,
                span,
                String::from(
                    "cannot determine the record type of this expression; add a type annotation",
                ),
            )),
            other => Err(self.error(
                CheckCode::C0009,
                span,
                format!("cannot access field `{name}` on non-record type `{other}`"),
            )),
        }
    }

    /// Reports constructing an opaque type outside its declaring module
    /// (C0022), if `name` is such a foreign opaque constructor. Returns `None`
    /// when `name` is not a constructor, or is one this module may construct.
    fn opaque_construct_error(&mut self, name: &str, span: Span) -> Option<Aborted> {
        let message = opaque_violation(
            self.registry.ctor(name)?,
            self.current_module.as_ref(),
            "construct",
        )?;
        Some(self.error(CheckCode::C0022, span, message))
    }

    /// `{ field: value, … }` — later duplicates override earlier ones.
    fn infer_record(&mut self, record: &RecordLit) -> Checked<Type> {
        let mut fields = Vec::new();
        for field in record.fields() {
            let Some(name) = field.name() else {
                return Err(Aborted);
            };
            let Some(value) = field.value() else {
                return Err(Aborted);
            };
            let ty = self.infer_expr(&value)?;
            fields.push((Label::new(name), ty));
        }
        Ok(Type::record(fields))
    }
}

/// The operator token kind of a binary expression.
fn binop_kind(op: &BinOpExpr) -> Option<SyntaxKind> {
    op.syntax()
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .map(|token| token.kind())
        .find(|kind| {
            matches!(
                kind,
                SyntaxKind::PLUS
                    | SyntaxKind::MINUS
                    | SyntaxKind::STAR
                    | SyntaxKind::SLASH
                    | SyntaxKind::LT
                    | SyntaxKind::LE
                    | SyntaxKind::GT
                    | SyntaxKind::GE
                    | SyntaxKind::EQ_EQ
                    | SyntaxKind::BANG_EQ
                    | SyntaxKind::ANDAND
                    | SyntaxKind::OROR
            )
        })
}

/// The "outside its module" diagnostic for touching a foreign opaque
/// constructor `info`; `verb` is the action (`construct`/`destructure`).
/// `None` when `info` is transparent or owned by `current`. A module-less
/// owner (single-file checking) renders as the empty module name.
fn opaque_violation(info: &CtorInfo, current: Option<&ModuleName>, verb: &str) -> Option<String> {
    if !info.opaque || info.module.as_ref() == current {
        return None;
    }
    let module = info
        .module
        .as_ref()
        .map_or_else(String::new, |m| m.to_string());
    Some(format!(
        "cannot {verb} opaque type `{}` outside module `{module}`",
        info.owner
    ))
}

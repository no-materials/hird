// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Actor declaration checking: typed mailboxes, init signatures, message
//! handlers with exhaustive coverage, and per-actor effect summaries.
//!
//! An actor declares a `state` type, a `message` sum type (registered as an
//! ordinary ADT so any code can construct messages), an `init` function
//! producing the initial state, and one `handle` clause per message
//! constructor. Checking runs in two phases around function checking:
//! registration (the actor's interface — message ADT, state type, init
//! signature — so any body may `spawn` it) and body checking (init and
//! handler bodies, after the functions they call have their schemes).
//!
//! The state value is encapsulated by construction: no expression form
//! reaches an actor's state from outside, and the actor's name is not a
//! value — referencing it as one is an error ([`CheckCode::C0040`]).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_ast::{ActorDecl, ActorField, ActorHandler, AstNode, Pattern, TypeExpr};
use hird_lex::Span;
use hird_types::{EffectRow, Name, Type, TypeError, unify_row};

use crate::checker::{Aborted, Checked, Checker};
use crate::diag::{CheckCode, CheckDiagnostic};
use crate::elaborate::Scope;
use crate::registry::CtorInfo;
use crate::{NodeKey, expr_span, name_token_span, node_span, type_expr_span};

/// A registered actor: the interface `spawn` and encapsulation checks consult.
#[derive(Debug, Clone)]
pub(crate) struct ActorInfo {
    /// The mailbox's message type name; `spawn` returns `Pid<message>`.
    pub(crate) message: Name,
    /// The declared state type.
    pub(crate) state: Type,
    /// The init function's parameter types, in order — what `spawn`'s
    /// arguments are checked against.
    pub(crate) init_params: Vec<Type>,
    /// The init function's declared effect row.
    pub(crate) init_row: EffectRow,
}

impl Checker {
    /// Registers an actor's message type as an ADT header (name, arity 0,
    /// constructor names), so constructor fields anywhere — including other
    /// actors' messages — can reference it.
    pub(crate) fn register_actor_message_header(&mut self, decl: &ActorDecl) {
        let Some(field) = actor_field(decl, "message") else {
            return;
        };
        let Some(name) = message_type_name(&field) else {
            return;
        };
        let ctors = field
            .constructors()
            .filter_map(|c| c.name().map(Name::new))
            .collect();
        self.registry.declare_adt(Name::new(name), 0, ctors);
    }

    /// Records an actor's message type and constructors in the type and value
    /// namespaces for duplicate detection. The actor's own name is noted
    /// separately, in the actor namespace.
    pub(crate) fn detect_actor_message_duplicates(&mut self, decl: &ActorDecl) {
        let Some(field) = actor_field(decl, "message") else {
            return;
        };
        if let Some(TypeExpr::Name(name)) = field.ty() {
            let text = String::from(name.text());
            let span = crate::token_span(name.syntax(), self.source_id);
            self.note_type_name(&text, span);
        }
        for ctor in field.constructors() {
            if let Some(name) = ctor.name() {
                let span = name_token_span(ctor.syntax(), self.source_id);
                self.note_value_name(name, span);
            }
        }
    }

    /// Registers an actor's interface: validates the member structure,
    /// elaborates the message constructors, the state type, and the init
    /// signature, and files the result in the actor namespace.
    pub(crate) fn register_actor(&mut self, decl: &ActorDecl) -> Checked<()> {
        let Some(name) = decl.name() else {
            return Ok(());
        };
        let decl_span = name_token_span(decl.syntax(), self.source_id);
        self.check_actor_members(decl, name)?;

        let Some(message_field) = actor_field(decl, "message") else {
            return Err(self.error(
                CheckCode::C0035,
                decl_span,
                format!("actor `{name}` is missing its `message` member"),
            ));
        };
        let Some(state_field) = actor_field(decl, "state") else {
            return Err(self.error(
                CheckCode::C0035,
                decl_span,
                format!("actor `{name}` is missing its `state` member"),
            ));
        };
        let Some(init_field) = actor_field(decl, "init") else {
            return Err(self.error(
                CheckCode::C0035,
                decl_span,
                format!("actor `{name}` is missing its `init` member"),
            ));
        };

        let message = self.register_message(&message_field, name)?;
        let state = self.register_state(&state_field, name)?;
        let (init_params, init_row) = self.register_init(&init_field, name, &state)?;

        // First declaration wins, consistent with duplicate reporting: a
        // duplicate actor (already reported) must not re-key the original's
        // interface out from under its own body check.
        self.actors.entry(String::from(name)).or_insert(ActorInfo {
            message,
            state,
            init_params,
            init_row,
        });
        Ok(())
    }

    /// Validates the member list: every field is one of `state`, `message`,
    /// `init`, and none repeats.
    fn check_actor_members(&mut self, decl: &ActorDecl, name: &str) -> Checked<()> {
        let mut seen: BTreeMap<String, Span> = BTreeMap::new();
        let mut result = Ok(());
        for field in decl.fields() {
            let Some(member) = field.name().map(String::from) else {
                continue;
            };
            let span = name_token_span(field.syntax(), self.source_id);
            if !matches!(member.as_str(), "state" | "message" | "init") {
                result = Err(self.error(
                    CheckCode::C0035,
                    span,
                    format!(
                        "actor `{name}` has no member `{member}`; \
                         expected `state`, `message`, `init`, or `handle`"
                    ),
                ));
                continue;
            }
            if let Some(first) = seen.get(&member).copied() {
                self.diags.push(
                    CheckDiagnostic::error(
                        CheckCode::C0035,
                        span,
                        format!("actor `{name}` declares `{member}` twice"),
                    )
                    .with_related(first, String::from("first declared here")),
                );
                result = Err(Aborted);
            } else {
                seen.insert(member, span);
            }
        }
        result
    }

    /// Elaborates and registers the message constructors as values, exactly as
    /// an ADT's constructors are: senders construct messages, so the
    /// constructors are public interface even though the state is not.
    fn register_message(&mut self, field: &ActorField, actor: &str) -> Checked<Name> {
        let span = name_token_span(field.syntax(), self.source_id);
        let Some(type_name) = message_type_name(field) else {
            return Err(self.error(
                CheckCode::C0035,
                span,
                format!(
                    "actor `{actor}`'s message member must be a named sum type, \
                     e.g. `message: Msg = A | B`"
                ),
            ));
        };
        let owner = Name::new(type_name.as_str());
        let result_ty = Type::con(type_name.as_str(), Vec::new());
        let mut scope = Scope::new();
        for ctor in field.constructors() {
            let Some(ctor_name) = ctor.name() else {
                continue;
            };
            let mut fields = Vec::new();
            for f in ctor.fields() {
                fields.push(self.elaborate_closed(&f, &mut scope)?);
            }
            let ctor_ty = if fields.is_empty() {
                result_ty.clone()
            } else {
                Type::func(fields, result_ty.clone())
            };
            self.registry.declare_ctor(
                Name::new(ctor_name),
                CtorInfo {
                    scheme: ctor_ty.clone(),
                    owner: owner.clone(),
                    module: self.current_module.clone(),
                    opaque: false,
                },
            );
            self.env.insert_root(ctor_name, ctor_ty.clone());
            self.types
                .push((NodeKey::of_node(ctor.syntax()), ctor_ty.clone()));
            self.bindings.push((String::from(ctor_name), ctor_ty));
        }
        Ok(owner)
    }

    /// Elaborates the state type, recording it for the IR.
    fn register_state(&mut self, field: &ActorField, actor: &str) -> Checked<Type> {
        let span = name_token_span(field.syntax(), self.source_id);
        let Some(ty_expr) = field.ty() else {
            return Err(self.error(
                CheckCode::C0035,
                span,
                format!("actor `{actor}`'s state member must be a type"),
            ));
        };
        let mut scope = Scope::new();
        let state = self.elaborate_closed(&ty_expr, &mut scope)?;
        self.types
            .push((NodeKey::of_node(field.syntax()), state.clone()));
        Ok(state)
    }

    /// Elaborates the init signature: parameter types (recorded as
    /// capabilities so the row may reference them), a return type that must be
    /// the state type, and the declared row. The body is checked later, in
    /// [`Checker::check_actor`].
    fn register_init(
        &mut self,
        field: &ActorField,
        actor: &str,
        state: &Type,
    ) -> Checked<(Vec<Type>, EffectRow)> {
        let span = name_token_span(field.syntax(), self.source_id);
        let Some(sig) = field.fn_sig() else {
            return Err(self.error(
                CheckCode::C0035,
                span,
                format!(
                    "actor `{actor}`'s init member must be a function, \
                     e.g. `init: fn(c: Config) \u{2192} State ! {{}} = e`"
                ),
            ));
        };
        if field.body().is_none() {
            return Err(self.error(
                CheckCode::C0035,
                span,
                format!("actor `{actor}`'s init is missing its `= body`"),
            ));
        }
        let mut scope = Scope::new();
        let mut params = Vec::new();
        for param in sig.params() {
            let Some(ty_expr) = param.ty() else {
                return Err(Aborted);
            };
            let ty = self.elaborate_closed(&ty_expr, &mut scope)?;
            if let Some(param_name) = param.name() {
                scope.insert_cap(param_name, ty.clone());
            }
            params.push(ty);
        }
        if let Some(ret) = sig.return_type() {
            let ret_ty = self.elaborate_closed(&ret, &mut scope)?;
            self.unify_at(state, &ret_ty, type_expr_span(&ret, self.source_id))?;
        }
        let row = match sig.effect_ann() {
            Some(ann) => self.elaborate_row_closed(&ann, &mut scope)?,
            None => EffectRow::empty(),
        };
        self.types.push((
            NodeKey::of_node(sig.syntax()),
            Type::func_eff(params.clone(), state.clone(), row.clone()),
        ));
        self.effect_rows
            .push((NodeKey::of_node(sig.syntax()), row.clone()));
        Ok((params, row))
    }

    /// Checks an actor's bodies: the init body against the state type and
    /// declared row, each handler against the message and state types, the
    /// handlers' coverage of the message constructors, and the declared
    /// effect summary against the union of member rows. Runs after function
    /// checking, so bodies see final function schemes.
    pub(crate) fn check_actor(&mut self, decl: &ActorDecl) -> Checked<()> {
        let Some(name) = decl.name() else {
            return Ok(());
        };
        // Registration failed and reported; nothing further to check against.
        let Some(info) = self.actors.get(name).cloned() else {
            return Ok(());
        };

        if let Some(init_field) = actor_field(decl, "init") {
            let _ = self.check_init_body(&init_field, &info);
        }

        let mut member_rows = info.init_row.clone();
        let mut seen: BTreeMap<String, Span> = BTreeMap::new();
        for handler in decl.handlers() {
            let _ = self.check_handler(&handler, name, &info, &mut seen, &mut member_rows);
        }

        self.check_handler_coverage(decl, name, &info, &seen);
        self.check_effect_summary(decl, name, &member_rows);
        Ok(())
    }

    /// Checks that the handlers cover every constructor of the message type.
    ///
    /// Each handler names exactly one known constructor with no duplicates
    /// (enforced above), so coverage is a set difference over the message
    /// type's constructor list — not the match-usefulness matrix, which still
    /// checks the payload patterns inside each handler.
    fn check_handler_coverage(
        &mut self,
        decl: &ActorDecl,
        actor: &str,
        info: &ActorInfo,
        seen: &BTreeMap<String, Span>,
    ) {
        let missing: Vec<String> = match self.registry.adt_constructors(info.message.as_str()) {
            Some(ctors) => ctors
                .iter()
                .filter(|c| !seen.contains_key(c.as_str()))
                .map(|c| String::from(c.as_str()))
                .collect(),
            None => return,
        };
        if missing.is_empty() {
            return;
        }
        let span = name_token_span(decl.syntax(), self.source_id);
        let noun = if missing.len() == 1 {
            "variant"
        } else {
            "variants"
        };
        self.diags.push(CheckDiagnostic::error(
            CheckCode::C0041,
            span,
            format!(
                "actor `{actor}` does not handle message {noun} `{}`",
                missing.join("`, `")
            ),
        ));
    }

    /// Checks the init body against the state type and the declared init row.
    fn check_init_body(&mut self, field: &ActorField, info: &ActorInfo) -> Checked<()> {
        let (Some(sig), Some(body)) = (field.fn_sig(), field.body()) else {
            return Ok(());
        };
        self.env.push_scope();
        for (param, ty) in sig.params().zip(info.init_params.clone()) {
            self.bind_param(&param, ty);
        }
        self.begin_effect_scope();
        let body_ty = self.infer_expr(&body);
        let inferred = self.take_effect_row();
        self.env.pop_scope();
        let body_ty = body_ty?;
        let span = expr_span(&body, self.source_id);
        self.unify_at(&info.state, &body_ty, span)?;
        self.check_effect_row(&info.init_row, &inferred, span);
        Ok(())
    }

    /// Checks one `handle` clause: the message pattern names a constructor of
    /// the message type (no duplicates), the state pattern binds the state
    /// type, the body produces the next state, and the body's effects equal
    /// the handler's declared row. The declared row joins `member_rows` for
    /// the actor-level summary check.
    fn check_handler(
        &mut self,
        handler: &ActorHandler,
        actor: &str,
        info: &ActorInfo,
        seen: &mut BTreeMap<String, Span>,
        member_rows: &mut EffectRow,
    ) -> Checked<()> {
        let Some(pattern) = handler.message_pattern() else {
            return Err(Aborted);
        };
        let pattern_span = node_span(pattern.syntax(), self.source_id);
        let Pattern::Constructor(ctor_pat) = &pattern else {
            return Err(self.error(
                CheckCode::C0037,
                pattern_span,
                format!("a handler in actor `{actor}` must name a message constructor"),
            ));
        };
        let Some(ctor_name) = ctor_pat.name() else {
            return Err(Aborted);
        };
        // A known constructor of the wrong type gets a tailored diagnostic;
        // an unknown one falls through to the pattern check's C0007.
        if let Some(ctor_info) = self.registry.ctor(ctor_name)
            && ctor_info.owner != info.message
        {
            return Err(self.error(
                CheckCode::C0037,
                pattern_span,
                format!(
                    "`{ctor_name}` is not a constructor of actor `{actor}`'s \
                     message type `{}`",
                    info.message
                ),
            ));
        }
        if let Some(first) = seen.get(ctor_name).copied() {
            self.diags.push(
                CheckDiagnostic::error(
                    CheckCode::C0036,
                    pattern_span,
                    format!("actor `{actor}` already handles `{ctor_name}`"),
                )
                .with_related(first, String::from("first handled here")),
            );
            return Err(Aborted);
        }
        seen.insert(String::from(ctor_name), pattern_span);

        // The declared row is recorded (and joins the summary) even when the
        // body later fails, so one bad handler does not cascade into a
        // spurious summary mismatch.
        let mut scope = Scope::new();
        let declared = match handler.effect_ann() {
            Some(ann) => self.elaborate_row_closed(&ann, &mut scope),
            None => Ok(EffectRow::empty()),
        };
        if let Ok(row) = &declared {
            self.effect_rows
                .push((NodeKey::of_node(handler.syntax()), row.clone()));
            for effect in row.effects() {
                member_rows.insert(effect.clone());
            }
        }

        let message_ty = Type::con(info.message.as_str(), Vec::new());
        self.env.push_scope();
        let mut result = self.check_pattern(&pattern, &message_ty);
        if result.is_ok() {
            result = match handler.state_pattern() {
                Some(state_pat) => self.check_pattern(&state_pat, &info.state),
                // The parser already reported the missing state pattern.
                None => Err(Aborted),
            };
        }
        if result.is_ok()
            && let Some(ret) = handler.return_type()
        {
            let span = type_expr_span(&ret, self.source_id);
            result = self
                .elaborate_closed(&ret, &mut scope)
                .and_then(|ret_ty| self.unify_at(&info.state, &ret_ty, span));
        }
        let body_res = match (&result, handler.body()) {
            (Ok(()), Some(body)) => {
                self.begin_effect_scope();
                let body_ty = self.infer_expr(&body);
                let inferred = self.take_effect_row();
                body_ty.map(|ty| (ty, inferred, expr_span(&body, self.source_id)))
            }
            _ => Err(Aborted),
        };
        self.env.pop_scope();
        result?;
        let (body_ty, inferred, body_span) = body_res?;
        self.unify_at(&info.state, &body_ty, body_span)?;
        if let Ok(declared) = declared {
            self.check_effect_row(&declared, &inferred, body_span);
        }
        Ok(())
    }

    /// Checks the actor's declared effect summary (the trailing `! { … }`, or
    /// the empty row when absent) against the union of the init row and every
    /// handler row.
    fn check_effect_summary(&mut self, decl: &ActorDecl, actor: &str, member_rows: &EffectRow) {
        let mut scope = Scope::new();
        let declared = match decl.effect_ann() {
            Some(ann) => match self.elaborate_row_closed(&ann, &mut scope) {
                Ok(row) => row,
                // The elaboration error is already reported; comparing against
                // a half-built row would only cascade.
                Err(Aborted) => return,
            },
            None => EffectRow::empty(),
        };
        self.effect_rows
            .push((NodeKey::of_node(decl.syntax()), declared.clone()));
        let span = name_token_span(decl.syntax(), self.source_id);
        if let Err(err) = unify_row(&mut self.subst, &declared, member_rows, span) {
            match err {
                TypeError::EffectMismatch { .. } => {
                    let declared = self.subst.resolve_row(&declared);
                    let performed = self.subst.resolve_row(member_rows);
                    self.diags.push(CheckDiagnostic::error(
                        CheckCode::C0038,
                        span,
                        format!(
                            "actor `{actor}` declares `{declared}` but its init and \
                             handlers perform `{performed}`"
                        ),
                    ));
                }
                other => self.diags.push(CheckDiagnostic::from_type_error(&other)),
            }
        }
    }
}

/// The actor body field named `member`, if present.
fn actor_field(decl: &ActorDecl, member: &str) -> Option<ActorField> {
    decl.fields().find(|f| f.name() == Some(member))
}

/// The message field's declared type name (`message: Msg = …`). `None` when
/// the field's value is missing, lowercase, or not a bare name.
fn message_type_name(field: &ActorField) -> Option<String> {
    match field.ty()? {
        TypeExpr::Name(name) if name.text().chars().next().is_some_and(char::is_uppercase) => {
            Some(String::from(name.text()))
        }
        _ => None,
    }
}

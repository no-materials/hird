// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Supervisor declaration checking: a closed set of body fields, typed child
//! specs, and a derived effect row.
//!
//! A supervisor declares a restart `strategy`, an `intensity`/`period` restart
//! budget, and a list of `children`. Each child names a declared actor, a
//! `start_args` expression checked against that actor's sole init parameter,
//! and a restart disposition. Start arguments are pure but for acquiring the
//! clock capability (`clock()`, effect `Clock`): a child spec is where a
//! supervised actor is handed its capabilities. The supervisor performs no
//! other effects of its own; its effect row is derived as the union of its
//! children's per-actor effect summaries plus what their start arguments
//! acquire, and recorded for the IR.
//!
//! Only `one_for_one` is implemented in v0.1: `one_for_all` and `rest_for_one`
//! parse and type-check but raise a warning ([`CheckCode::C0050`]).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_ast::{AstNode, Expr, RecordField, RecordLit, SupervisorDecl, SupervisorField};
use hird_lex::Span;
use hird_parse::SyntaxKind;
use hird_types::EffectRow;

use crate::checker::Checker;
use crate::diag::{CheckCode, CheckDiagnostic};
use crate::{NodeKey, expr_span, name_token_span, node_span};

/// The restart strategies the surface accepts. Only `one_for_one` is lowered in
/// v0.1; the rest warn.
const STRATEGIES: [&str; 3] = ["one_for_one", "one_for_all", "rest_for_one"];

/// The restart dispositions a child spec accepts.
const RESTARTS: [&str; 3] = ["permanent", "temporary", "transient"];

/// A registered supervisor: the interface `supervise` and `child` resolve
/// against.
#[derive(Debug, Clone)]
pub(crate) struct SupervisorInfo {
    /// Declared children as `(id, actor name)` pairs, in source order.
    pub(crate) children: Vec<(String, String)>,
}

impl Checker {
    /// Registers a supervisor's interface — its name and `(child id, actor)`
    /// pairs — before function checking, so any body may `supervise` it and
    /// look up its children. Skims the declaration structurally: malformed
    /// fields are skipped here and reported by [`Checker::check_supervisor`],
    /// which runs after function checking.
    pub(crate) fn register_supervisor(&mut self, decl: &SupervisorDecl) {
        let Some(name) = decl.name() else {
            return;
        };
        let mut children = Vec::new();
        let specs = decl
            .fields()
            .find(|f| f.name() == Some("children"))
            .and_then(|f| f.value());
        if let Some(Expr::List(list)) = specs {
            for elem in list.elements() {
                let Expr::Record(spec) = elem else {
                    continue;
                };
                let field = |name: &str| {
                    spec.fields()
                        .find(|f| f.name() == Some(name))
                        .and_then(|f| match f.value() {
                            Some(Expr::Name(n)) => Some(String::from(n.text())),
                            _ => None,
                        })
                };
                if let (Some(id), Some(actor)) = (field("id"), field("actor")) {
                    children.push((id, actor));
                }
            }
        }
        self.supervisors
            .insert(String::from(name), SupervisorInfo { children });
    }

    /// Checks a supervisor declaration: validates the closed field set, each
    /// child spec, and records the derived effect row for the IR. Runs after
    /// function and actor checking, so `start_args` sees final schemes and
    /// children resolve against registered actors.
    pub(crate) fn check_supervisor(&mut self, decl: &SupervisorDecl) {
        let Some(name) = decl.name() else {
            return;
        };
        let name = String::from(name);
        let decl_span = name_token_span(decl.syntax(), self.source_id);

        // Gather the body fields, enforcing the closed set and rejecting
        // duplicates against the first occurrence.
        let mut fields: BTreeMap<String, SupervisorField> = BTreeMap::new();
        let mut seen: BTreeMap<String, Span> = BTreeMap::new();
        for field in decl.fields() {
            let Some(field_name) = field.name().map(String::from) else {
                continue;
            };
            let span = name_token_span(field.syntax(), self.source_id);
            if !matches!(
                field_name.as_str(),
                "strategy" | "intensity" | "period" | "children"
            ) {
                self.error(
                    CheckCode::C0046,
                    span,
                    format!(
                        "supervisor `{name}` has no field `{field_name}`; \
                         expected `strategy`, `intensity`, `period`, or `children`"
                    ),
                );
                continue;
            }
            if let Some(first) = seen.get(&field_name).copied() {
                self.diags.push(
                    CheckDiagnostic::error(
                        CheckCode::C0046,
                        span,
                        format!("supervisor `{name}` declares `{field_name}` twice"),
                    )
                    .with_related(first, String::from("first declared here")),
                );
                continue;
            }
            seen.insert(field_name.clone(), span);
            fields.insert(field_name, field);
        }

        match fields.get("strategy") {
            Some(field) => self.check_strategy(&name, field),
            None => {
                self.error(
                    CheckCode::C0046,
                    decl_span,
                    format!("supervisor `{name}` is missing its `strategy` field"),
                );
            }
        }
        self.check_positive_int(&name, fields.get("intensity"), "intensity", decl_span);
        self.check_positive_int(&name, fields.get("period"), "period", decl_span);

        let derived = match fields.get("children") {
            Some(field) => self.check_children(&name, field),
            None => {
                self.error(
                    CheckCode::C0046,
                    decl_span,
                    format!("supervisor `{name}` is missing its `children` field"),
                );
                EffectRow::empty()
            }
        };
        // The derived row is recorded even when a child errored: it is purely a
        // function of the resolvable children, and the IR reads it back.
        self.effect_rows
            .push((NodeKey::of_node(decl.syntax()), derived));
    }

    /// Checks the `strategy` field names a known strategy; `one_for_all` and
    /// `rest_for_one` warn as unimplemented in v0.1.
    fn check_strategy(&mut self, sup: &str, field: &SupervisorField) {
        let Some(value) = field.value() else {
            return;
        };
        let span = expr_span(&value, self.source_id);
        let Expr::Name(strategy) = &value else {
            self.error(
                CheckCode::C0046,
                span,
                format!(
                    "supervisor `{sup}`'s `strategy` must be one of \
                     `one_for_one`, `one_for_all`, or `rest_for_one`"
                ),
            );
            return;
        };
        let strategy = strategy.text();
        if !STRATEGIES.contains(&strategy) {
            self.error(
                CheckCode::C0046,
                span,
                format!(
                    "supervisor `{sup}` has unknown restart strategy `{strategy}`; \
                     expected `one_for_one`, `one_for_all`, or `rest_for_one`"
                ),
            );
        } else if strategy != "one_for_one" {
            self.diags.push(CheckDiagnostic::warning(
                CheckCode::C0050,
                span,
                format!(
                    "restart strategy `{strategy}` is not implemented yet; \
                     only `one_for_one` is supported"
                ),
            ));
        }
    }

    /// Checks an `intensity`/`period` field is a positive integer literal. A
    /// missing field is reported against the declaration.
    fn check_positive_int(
        &mut self,
        sup: &str,
        field: Option<&SupervisorField>,
        what: &str,
        decl_span: Span,
    ) {
        let Some(field) = field else {
            self.error(
                CheckCode::C0046,
                decl_span,
                format!("supervisor `{sup}` is missing its `{what}` field"),
            );
            return;
        };
        let Some(value) = field.value() else {
            return;
        };
        let span = expr_span(&value, self.source_id);
        let valid = matches!(
            &value,
            Expr::Literal(lit)
                if lit.kind() == SyntaxKind::INT
                    && lit.text().parse::<u32>().is_ok_and(|n| n > 0)
        );
        if !valid {
            self.error(
                CheckCode::C0046,
                span,
                format!("supervisor `{sup}`'s `{what}` must be a positive integer"),
            );
        }
    }

    /// Checks the `children` field is a list of child-spec records, returning
    /// the union of the resolvable children's per-actor effect summaries.
    fn check_children(&mut self, sup: &str, field: &SupervisorField) -> EffectRow {
        let mut derived = EffectRow::empty();
        let Some(value) = field.value() else {
            return derived;
        };
        let Expr::List(list) = &value else {
            self.error(
                CheckCode::C0046,
                expr_span(&value, self.source_id),
                format!(
                    "supervisor `{sup}`'s `children` must be a list of child specs, \
                     e.g. `[ {{ id: c, actor: A, start_args: e, restart: permanent }} ]`"
                ),
            );
            return derived;
        };
        let mut ids: BTreeMap<String, Span> = BTreeMap::new();
        for elem in list.elements() {
            let Expr::Record(spec) = &elem else {
                self.error(
                    CheckCode::C0046,
                    expr_span(&elem, self.source_id),
                    format!(
                        "a child of supervisor `{sup}` must be a record \
                         `{{ id, actor, start_args, restart }}`"
                    ),
                );
                continue;
            };
            if let Some(row) = self.check_child_spec(sup, spec, &mut ids) {
                for effect in row.effects() {
                    derived.insert(effect.clone());
                }
            }
        }
        derived
    }

    /// Checks one child spec: the closed field set, a unique lowercase `id`, an
    /// `actor` resolving to a declared actor, a valid `restart`, and pure
    /// `start_args` matching the actor's sole init parameter. Returns the
    /// resolved actor's per-actor effect summary (its contribution to the
    /// supervisor's derived row), or `None` when the actor does not resolve.
    fn check_child_spec(
        &mut self,
        sup: &str,
        spec: &RecordLit,
        ids: &mut BTreeMap<String, Span>,
    ) -> Option<EffectRow> {
        let spec_span = node_span(spec.syntax(), self.source_id);

        // Gather the child fields, enforcing the closed set and rejecting
        // duplicates. `actor` is lexed as a keyword, so field spans point at the
        // whole `name: value` field rather than an `IDENT` token.
        let mut fields: BTreeMap<String, RecordField> = BTreeMap::new();
        let mut seen: BTreeMap<String, Span> = BTreeMap::new();
        for f in spec.fields() {
            let Some(field_name) = f.name().map(String::from) else {
                continue;
            };
            let span = node_span(f.syntax(), self.source_id);
            if !matches!(
                field_name.as_str(),
                "id" | "actor" | "start_args" | "restart"
            ) {
                self.error(
                    CheckCode::C0046,
                    span,
                    format!(
                        "a child of supervisor `{sup}` has no field `{field_name}`; \
                         expected `id`, `actor`, `start_args`, or `restart`"
                    ),
                );
                continue;
            }
            if let Some(first) = seen.get(&field_name).copied() {
                self.diags.push(
                    CheckDiagnostic::error(
                        CheckCode::C0046,
                        span,
                        format!("a child of supervisor `{sup}` declares `{field_name}` twice"),
                    )
                    .with_related(first, String::from("first declared here")),
                );
                continue;
            }
            seen.insert(field_name.clone(), span);
            fields.insert(field_name, f);
        }

        for required in ["id", "actor", "start_args", "restart"] {
            if !fields.contains_key(required) {
                self.error(
                    CheckCode::C0046,
                    spec_span,
                    format!("a child of supervisor `{sup}` is missing its `{required}` field"),
                );
            }
        }

        let id = self.check_child_id(sup, fields.get("id"), ids);
        let label = id.clone().unwrap_or_else(|| String::from("<child>"));
        let actor = self.resolve_child_actor(sup, &label, fields.get("actor"));
        self.check_restart(sup, &label, fields.get("restart"));
        let acquired = self.check_start_args(sup, &label, fields.get("start_args"), actor.as_ref());

        actor.and_then(|(_, info)| info.summary).map(|mut row| {
            for effect in acquired.effects() {
                row.insert(effect.clone());
            }
            row
        })
    }

    /// Checks a child's `id` is a bare lowercase identifier, unique within the
    /// supervisor. Returns the id text when valid.
    fn check_child_id(
        &mut self,
        sup: &str,
        field: Option<&RecordField>,
        ids: &mut BTreeMap<String, Span>,
    ) -> Option<String> {
        let value = field?.value()?;
        let span = expr_span(&value, self.source_id);
        let Expr::Name(id) = &value else {
            self.error(
                CheckCode::C0046,
                span,
                format!("supervisor `{sup}`'s child `id` must be a bare identifier"),
            );
            return None;
        };
        let id = id.text();
        if id.starts_with(|c: char| c.is_uppercase()) {
            self.error(
                CheckCode::C0046,
                span,
                format!("supervisor `{sup}`'s child `id` `{id}` must be lowercase"),
            );
            return None;
        }
        if let Some(first) = ids.get(id).copied() {
            self.diags.push(
                CheckDiagnostic::error(
                    CheckCode::C0046,
                    span,
                    format!("supervisor `{sup}` declares two children with id `{id}`"),
                )
                .with_related(first, String::from("first declared here")),
            );
            return None;
        }
        ids.insert(String::from(id), span);
        Some(String::from(id))
    }

    /// Resolves a child's `actor` field to a declared actor. Returns the actor
    /// name and its interface when it resolves.
    fn resolve_child_actor(
        &mut self,
        sup: &str,
        child: &str,
        field: Option<&RecordField>,
    ) -> Option<(String, crate::actors::ActorInfo)> {
        let value = field?.value()?;
        let span = expr_span(&value, self.source_id);
        let Expr::Name(actor) = &value else {
            self.error(
                CheckCode::C0046,
                span,
                format!("supervisor `{sup}`'s child `{child}` `actor` must be an actor name"),
            );
            return None;
        };
        let actor = String::from(actor.text());
        match self.actors.get(&actor).cloned() {
            Some(info) => Some((actor, info)),
            None => {
                self.error(
                    CheckCode::C0047,
                    span,
                    format!(
                        "supervisor `{sup}`'s child `{child}` references undeclared actor `{actor}`"
                    ),
                );
                None
            }
        }
    }

    /// Checks a child's `restart` field names a valid restart disposition.
    fn check_restart(&mut self, sup: &str, child: &str, field: Option<&RecordField>) {
        let Some(value) = field.and_then(RecordField::value) else {
            return;
        };
        let span = expr_span(&value, self.source_id);
        let valid = matches!(&value, Expr::Name(r) if RESTARTS.contains(&r.text()));
        if !valid {
            self.error(
                CheckCode::C0046,
                span,
                format!(
                    "supervisor `{sup}`'s child `{child}` has invalid `restart`; \
                     expected `permanent`, `temporary`, or `transient`"
                ),
            );
        }
    }

    /// Checks a child's `start_args` against the actor's sole init parameter and
    /// that it is pure but for acquiring the clock (`Clock` is the one effect
    /// allowed: it needs no handler, and a child spec is where a supervised
    /// actor gets its capabilities). Returns the acquired row for the
    /// supervisor's derived row. Always infers the expression (recording its
    /// node types for the IR); the match and purity checks apply only once the
    /// actor resolves with a single init parameter.
    fn check_start_args(
        &mut self,
        sup: &str,
        child: &str,
        field: Option<&RecordField>,
        actor: Option<&(String, crate::actors::ActorInfo)>,
    ) -> EffectRow {
        let Some(value) = field.and_then(RecordField::value) else {
            return EffectRow::empty();
        };
        let span = expr_span(&value, self.source_id);
        self.begin_effect_scope();
        let inferred = self.infer_expr(&value);
        let row = self.take_effect_row();

        let Some((actor_name, info)) = actor else {
            return EffectRow::empty();
        };
        if info.init_params.len() != 1 {
            self.error(
                CheckCode::C0048,
                span,
                format!(
                    "actor `{actor_name}` cannot be a supervised child: its init takes {} \
                     parameters, but a supervised actor's init must take exactly one",
                    info.init_params.len()
                ),
            );
            return EffectRow::empty();
        }
        if let Ok(ty) = inferred {
            let expected = info.init_params[0].clone();
            let _ = self.unify_at(&expected, &ty, span);
        }
        let row = self.subst.resolve_row(&row);
        let acquires_only = row.tail().is_none()
            && row
                .effects()
                .all(|e| e.head().as_str() == "Clock" && e.args().is_empty());
        if !acquires_only {
            self.error(
                CheckCode::C0049,
                span,
                format!(
                    "supervisor `{sup}`'s child `{child}` has effectful `start_args`; \
                     start arguments run during supervisor init and must be pure \
                     (acquiring the clock with `clock()` is the one exception)"
                ),
            );
            return EffectRow::empty();
        }
        row
    }
}

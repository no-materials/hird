// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The actor/effect graph: a versioned, serializable projection of an
//! [`IrModule`] for tooling (the CLI's `emit-effect-graph` and the MCP
//! server).
//!
//! The graph shows actors (message types, per-handler effect rows, the
//! declared summary), supervisors (strategy and children), and tool
//! declarations (argument/result types, declared trailing row). Types and
//! effect rows are rendered both structurally and as canonical
//! surface-syntax strings. The schema is versioned by
//! [`EFFECT_GRAPH_SCHEMA_VERSION`] and evolves additively only.

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_types::{Effect, EffectRow, Type};
use serde::Serialize;

use crate::ir::{
    IrActorDef, IrDecl, IrModule, IrParam, IrPattern, IrSupervisorDef, IrToolDef, IrTypeDef,
};

/// Version of the effect-graph schema. Bumped only for breaking changes;
/// additions are absorbed without a bump.
pub const EFFECT_GRAPH_SCHEMA_VERSION: u32 = 1;

/// The actor/effect graph of one module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EffectGraph {
    /// Schema version of this projection ([`EFFECT_GRAPH_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// The projected module's name.
    pub module: String,
    /// Actor declarations, in source order.
    pub actors: Vec<ActorNode>,
    /// Supervisor declarations, in source order.
    pub supervisors: Vec<SupervisorNode>,
    /// Tool declarations, in source order.
    pub tools: Vec<ToolNode>,
}

impl EffectGraph {
    /// Serializes the graph to compact JSON.
    ///
    /// # Errors
    ///
    /// Propagates any [`serde_json`] serialization error (none arise for
    /// well-formed graphs).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serializes the graph to indented JSON.
    ///
    /// # Errors
    ///
    /// Propagates any [`serde_json`] serialization error (none arise for
    /// well-formed graphs).
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// An actor: mailbox type, init, handlers, and its declared effect summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActorNode {
    /// The actor's name.
    pub name: String,
    /// 1-based source line of the declaration; 0 when unknown.
    pub line: u32,
    /// The state type.
    pub state: TypeRef,
    /// The mailbox message sum type.
    pub message: MessageNode,
    /// The init member.
    pub init: InitNode,
    /// One entry per message handler, in source order.
    pub handlers: Vec<HandlerNode>,
    /// The declared per-actor effect summary (union of init and handler rows).
    pub effects: EffectRowRef,
}

/// An actor's message sum type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MessageNode {
    /// The message type's name.
    pub name: String,
    /// The message constructors, in source order.
    pub constructors: Vec<ConstructorNode>,
}

/// One constructor of a message type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConstructorNode {
    /// The constructor's name.
    pub name: String,
    /// The field types, in order.
    pub fields: Vec<TypeRef>,
}

/// An actor's init member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InitNode {
    /// The init parameters.
    pub params: Vec<ParamNode>,
    /// The declared init effect row.
    pub effects: EffectRowRef,
}

/// A named, typed parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParamNode {
    /// The parameter's name.
    pub name: String,
    /// The parameter's type.
    #[serde(rename = "type")]
    pub ty: TypeRef,
}

/// One message handler of an actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HandlerNode {
    /// The handled message constructor's name.
    pub message: String,
    /// The declared handler effect row.
    pub effects: EffectRowRef,
}

/// A supervisor: strategy, restart budget, and supervised children.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SupervisorNode {
    /// The supervisor's name.
    pub name: String,
    /// 1-based source line of the declaration; 0 when unknown.
    pub line: u32,
    /// The restart strategy (`one_for_one`, `one_for_all`, `rest_for_one`).
    pub strategy: String,
    /// Maximum restarts within `period`.
    pub intensity: u32,
    /// The restart-intensity window, in seconds.
    pub period: u32,
    /// The supervised children, in source order.
    pub children: Vec<ChildNode>,
    /// The derived effect row (union of the children's actor summaries).
    pub effects: EffectRowRef,
}

/// One supervised child.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChildNode {
    /// The child's id.
    pub id: String,
    /// The supervised actor's name.
    pub actor: String,
    /// The restart mode (`permanent`, `temporary`, `transient`).
    pub restart: String,
}

/// A tool declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolNode {
    /// The tool's marker-type name.
    pub name: String,
    /// 1-based source line of the declaration; 0 when unknown.
    pub line: u32,
    /// Type-parameter names of a generic tool; empty otherwise.
    pub params: Vec<String>,
    /// The argument record type.
    pub input: TypeRef,
    /// The result type.
    pub output: TypeRef,
    /// The declared trailing row — the tool function's full row additionally
    /// carries the implicit `Tool<name>`.
    pub effects: EffectRowRef,
}

/// A type, rendered canonically and structurally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeRef {
    /// Canonical surface-syntax rendering (e.g. `List<Option<a>>`).
    pub display: String,
    /// The structural rendering.
    pub structure: TypeStructure,
}

/// The structural rendering of a type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum TypeStructure {
    /// A type variable.
    Var {
        /// The variable's display name (`a`, `b`, …).
        name: String,
    },
    /// A constructor applied to zero or more arguments.
    Con {
        /// The constructor's name.
        name: String,
        /// The type arguments, in order.
        args: Vec<Self>,
    },
    /// A function type.
    Fn {
        /// The parameter types, in order.
        params: Vec<Self>,
        /// The result type.
        result: Box<Self>,
        /// The function's effect row.
        effects: EffectRowRef,
    },
    /// A tuple type.
    Tuple {
        /// The element types, in order.
        elems: Vec<Self>,
    },
    /// A structural record type.
    Record {
        /// The field types, keyed by label.
        fields: BTreeMap<String, Self>,
    },
    /// A quantified type scheme.
    Forall {
        /// The quantified type variables' display names.
        vars: Vec<String>,
        /// The quantified row variables' display names.
        rows: Vec<String>,
        /// The quantified body.
        body: Box<Self>,
    },
}

/// An effect row, rendered canonically and structurally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EffectRowRef {
    /// Canonical surface-syntax rendering (e.g. `{Log, Tool<ReadRepo>}`).
    pub display: String,
    /// The row's effects, in head order.
    pub effects: Vec<EffectRef>,
    /// Whether the row is open (has a tail row variable).
    pub open: bool,
}

/// One effect of a row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EffectRef {
    /// Canonical surface-syntax rendering (e.g. `Tool<ReadRepo>`).
    pub display: String,
    /// The effect-constructor name (e.g. `Tool`).
    pub head: String,
    /// The effect's type arguments, in order.
    pub args: Vec<TypeRef>,
}

/// Projects `module`'s actor/effect graph.
#[must_use]
pub fn effect_graph(module: &IrModule) -> EffectGraph {
    let mut actors = Vec::new();
    let mut supervisors = Vec::new();
    let mut tools = Vec::new();
    for decl in &module.declarations {
        match decl {
            IrDecl::Actor(actor) => actors.push(actor_node(actor)),
            IrDecl::Supervisor(sup) => supervisors.push(supervisor_node(sup)),
            IrDecl::Tool(tool) => tools.push(tool_node(tool)),
            IrDecl::Fn(_) | IrDecl::Type(_) | IrDecl::Extern(_) => {}
        }
    }
    EffectGraph {
        schema_version: EFFECT_GRAPH_SCHEMA_VERSION,
        module: module.name.clone(),
        actors,
        supervisors,
        tools,
    }
}

/// Projects one actor declaration.
fn actor_node(actor: &IrActorDef) -> ActorNode {
    ActorNode {
        name: actor.name.clone(),
        line: actor.span.line,
        state: norm_type_ref(&actor.state),
        message: message_node(&actor.message),
        init: InitNode {
            params: actor.init.params.iter().map(param_node).collect(),
            effects: row_ref(&actor.init.effect_row),
        },
        handlers: actor
            .handlers
            .iter()
            .map(|h| HandlerNode {
                message: handled_constructor(&h.message),
                effects: row_ref(&h.effect_row),
            })
            .collect(),
        effects: row_ref(&actor.effect_row),
    }
}

/// Projects an actor's message sum type.
fn message_node(message: &IrTypeDef) -> MessageNode {
    MessageNode {
        name: message.name.clone(),
        constructors: message
            .constructors
            .iter()
            .map(|c| ConstructorNode {
                name: c.name.clone(),
                fields: c.fields.iter().map(norm_type_ref).collect(),
            })
            .collect(),
    }
}

/// The constructor name a handler's message pattern matches; `_` when the
/// pattern is not a constructor.
fn handled_constructor(pattern: &IrPattern) -> String {
    match pattern {
        IrPattern::Constructor(c) => c.name.clone(),
        IrPattern::Tuple(_)
        | IrPattern::Literal(_)
        | IrPattern::Wildcard(_)
        | IrPattern::Bind(_) => "_".to_owned(),
    }
}

/// Projects one supervisor declaration.
fn supervisor_node(sup: &IrSupervisorDef) -> SupervisorNode {
    SupervisorNode {
        name: sup.name.clone(),
        line: sup.span.line,
        strategy: sup.strategy.clone(),
        intensity: sup.intensity,
        period: sup.period,
        children: sup
            .children
            .iter()
            .map(|c| ChildNode {
                id: c.id.clone(),
                actor: c.actor.clone(),
                restart: c.restart.clone(),
            })
            .collect(),
        effects: row_ref(&sup.effect_row),
    }
}

/// Projects one tool declaration. The input, output, and declared row are
/// renumbered together (through a synthetic function type), so a generic
/// tool's type parameters render consistently across all three.
fn tool_node(tool: &IrToolDef) -> ToolNode {
    let signature = Type::func_eff(
        Vec::from([tool.input.clone()]),
        tool.output.clone(),
        tool.effect_row.clone(),
    )
    .normalized();
    let Type::TyFn(params, output, row) = signature else {
        // `func_eff` always yields `TyFn`, and `normalized` preserves shape.
        unreachable!("normalizing a function type yields a function type");
    };
    ToolNode {
        name: tool.name.clone(),
        line: tool.span.line,
        params: tool.params.clone(),
        input: type_ref(&params[0]),
        output: type_ref(&output),
        effects: row_ref(&row),
    }
}

/// Projects a parameter.
fn param_node(param: &IrParam) -> ParamNode {
    ParamNode {
        name: param.name.clone(),
        ty: norm_type_ref(&param.ty),
    }
}

/// A [`TypeRef`] of `ty`, renumbering its variables for canonical display.
fn norm_type_ref(ty: &Type) -> TypeRef {
    type_ref(&ty.normalized())
}

/// A [`TypeRef`] of an already-canonical `ty`.
fn type_ref(ty: &Type) -> TypeRef {
    TypeRef {
        display: format!("{ty}"),
        structure: structure(ty),
    }
}

/// The structural rendering of an already-canonical `ty`.
fn structure(ty: &Type) -> TypeStructure {
    match ty {
        Type::TyVar(v) => TypeStructure::Var {
            name: format!("{}", Type::var(*v)),
        },
        Type::TyCon(name, args) => TypeStructure::Con {
            name: format!("{name}"),
            args: args.iter().map(structure).collect(),
        },
        Type::TyFn(params, result, row) => TypeStructure::Fn {
            params: params.iter().map(structure).collect(),
            result: Box::new(structure(result)),
            effects: row_ref(row),
        },
        Type::TyTuple(elems) => TypeStructure::Tuple {
            elems: elems.iter().map(structure).collect(),
        },
        Type::TyRecord(fields) => TypeStructure::Record {
            fields: fields
                .iter()
                .map(|(label, field)| (format!("{label}"), structure(field)))
                .collect(),
        },
        Type::TyForall(vars, rows, body) => TypeStructure::Forall {
            vars: vars.iter().map(|v| format!("{}", Type::var(*v))).collect(),
            rows: rows.iter().map(|r| format!("{r}")).collect(),
            body: Box::new(structure(body)),
        },
    }
}

/// An [`EffectRowRef`] of an already-canonical `row`.
fn row_ref(row: &EffectRow) -> EffectRowRef {
    EffectRowRef {
        display: format!("{row}"),
        effects: row.effects().map(effect_ref).collect(),
        open: row.tail().is_some(),
    }
}

/// An [`EffectRef`] of an already-canonical `effect`.
fn effect_ref(effect: &Effect) -> EffectRef {
    EffectRef {
        display: format!("{effect}"),
        head: format!("{}", effect.head()),
        args: effect.args().iter().map(type_ref).collect(),
    }
}

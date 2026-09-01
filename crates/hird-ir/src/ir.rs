// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The IR node kinds and their JSON projection.
//!
//! The IR is a fully elaborated, structurally simplified form of the typed
//! AST: every node carries its resolved [`Type`], syntactic sugar is desugared
//! (`if` to `match`, operators to application), and parentheses are dropped.
//! Functions and applications are n-ary, matching the type system (BEAM
//! functions take an argument list; there is no auto-currying).
//!
//! # Serialization
//!
//! Every node derives [`serde::Serialize`]. Node enums tag the variant with a
//! `"kind"` field; embedded [`Type`]s render as their canonical textual form
//! (the same rendering as [`Type`]'s `Display`), so JSON stays readable for
//! tooling and the MCP server. The schema is documented in `docs/ir.md`.
//! Deserialization is intentionally not provided: the IR is produced by
//! lowering, never parsed back from JSON.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_types::{Effect, EffectRow, Type};
use serde::{Serialize, Serializer};

/// Serializes a [`Type`] as its canonical textual rendering (e.g. `List<Int>`,
/// `a → b`), keeping the JSON readable rather than a nested type tree.
fn serialize_type<S>(ty: &Type, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{ty}"))
}

/// Serializes an [`EffectRow`] as its textual rendering (e.g. `{}`, `{Log}`,
/// `{Log, Tool<X>}`, `{Log | r}`), mirroring how [`Type`] is serialized.
fn serialize_effect_row<S>(row: &EffectRow, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{row}"))
}

/// Serializes an [`Effect`] as its textual rendering (e.g. `Log`,
/// `Tool<ReadRepo>`), mirroring how [`Type`] is serialized.
fn serialize_effect<S>(effect: &Effect, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{effect}"))
}

/// A declaration's source position, for `%% <file>:<line>` comments in
/// generated Erlang.
///
/// Not serialized: the IR's JSON stays a semantic artifact, and positions
/// would churn it on every unrelated edit. `line` is 1-based; `0` means
/// unknown (an IR built by hand rather than by lowering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IrSpan {
    /// 1-based source line of the declaration's first token; 0 when unknown.
    pub line: u32,
}

// ── module and declarations ─────────────────────────────────────

/// A lowered module: a name and its declarations in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrModule {
    /// The module's name (path-derived; authoritative).
    pub name: String,
    /// Function, type, extern, tool, actor, and supervisor declarations, in
    /// source order. Imports are resolved away, and effect declarations are
    /// synthesised on printing rather than stored, so neither appears here.
    pub declarations: Vec<IrDecl>,
}

impl IrModule {
    /// Serializes the module to compact JSON.
    ///
    /// # Errors
    ///
    /// Propagates any [`serde_json`] serialization error (none arise for
    /// well-formed IR).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Serializes the module to indented JSON.
    ///
    /// # Errors
    ///
    /// Propagates any [`serde_json`] serialization error (none arise for
    /// well-formed IR).
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// A top-level declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum IrDecl {
    /// A function definition.
    Fn(IrFnDef),
    /// A data type definition.
    Type(IrTypeDef),
    /// A reference to an external (FFI) function.
    Extern(IrExternRef),
    /// A tool declaration.
    Tool(IrToolDef),
    /// An actor declaration.
    Actor(IrActorDef),
    /// A supervisor declaration.
    Supervisor(IrSupervisorDef),
}

/// A function definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrFnDef {
    /// The function's name.
    pub name: String,
    /// The parameters, in order, each with its resolved type.
    pub params: Vec<IrParam>,
    /// The result type.
    #[serde(serialize_with = "serialize_type")]
    pub return_type: Type,
    /// The effect row the function performs, from its declared annotation
    /// (empty when none is given; body inference is a later pass).
    #[serde(serialize_with = "serialize_effect_row")]
    pub effect_row: EffectRow,
    /// The body expression.
    pub body: IrExpr,
    /// The declaration's source position.
    #[serde(skip)]
    pub span: IrSpan,
}

/// A named, typed parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrParam {
    /// The parameter name.
    pub name: String,
    /// The parameter's resolved type.
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

/// A data type definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrTypeDef {
    /// The type's name.
    pub name: String,
    /// The type-parameter names, in declaration order.
    pub params: Vec<String>,
    /// The constructors, in declaration order.
    pub constructors: Vec<IrConstructorDef>,
    /// The declaration's source position.
    #[serde(skip)]
    pub span: IrSpan,
}

/// One constructor of a data type definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrConstructorDef {
    /// The constructor name.
    pub name: String,
    /// The field types, in order. Type-parameter variables render with their
    /// declared names (e.g. `a`, `List<a>`).
    #[serde(serialize_with = "serialize_types")]
    pub fields: Vec<Type>,
}

/// A tool declaration: an auditable external operation
/// (`tool Name<params> : args → result ! {row}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrToolDef {
    /// The tool's marker name (`ReadRepo` in `Tool<ReadRepo>`).
    pub name: String,
    /// The type-parameter names, in declaration order.
    pub params: Vec<String>,
    /// The operation's args record type. Type-parameter variables render with
    /// their declared names.
    #[serde(serialize_with = "serialize_type")]
    pub input: Type,
    /// The operation's result type.
    #[serde(serialize_with = "serialize_type")]
    pub output: Type,
    /// The declared trailing effect row; the implicit `Tool<name>` effect is
    /// not included.
    #[serde(serialize_with = "serialize_effect_row")]
    pub effect_row: EffectRow,
    /// The declaration's source position.
    #[serde(skip)]
    pub span: IrSpan,
}

/// An actor declaration: a typed mailbox, encapsulated state, an init
/// function, and one handler per message constructor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrActorDef {
    /// The actor's name (its own namespace; not a value).
    pub name: String,
    /// The declared state type. The state value is reachable only inside the
    /// handlers' state patterns.
    #[serde(serialize_with = "serialize_type")]
    pub state: Type,
    /// The typed mailbox: the message sum type, registered as an ordinary ADT
    /// so senders can construct messages.
    pub message: IrTypeDef,
    /// The init function producing the initial state.
    pub init: IrActorInit,
    /// The message handlers, in declaration order.
    pub handlers: Vec<IrActorHandler>,
    /// The declared per-actor effect summary: the union of the init row and
    /// every handler row.
    #[serde(serialize_with = "serialize_effect_row")]
    pub effect_row: EffectRow,
    /// The declaration's source position.
    #[serde(skip)]
    pub span: IrSpan,
}

/// An actor's init function (`init: fn(params) → State ! {row} = body`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrActorInit {
    /// The parameters `spawn` arguments are checked against, in order.
    pub params: Vec<IrParam>,
    /// The declared effect row; performed in the spawned process, not the
    /// spawner's.
    #[serde(serialize_with = "serialize_effect_row")]
    pub effect_row: EffectRow,
    /// The body producing the initial state.
    pub body: IrExpr,
}

/// One actor message handler
/// (`handle Ctor(payload), st → State ! {row} = body`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrActorHandler {
    /// The message pattern (a constructor of the actor's message type).
    pub message: IrPattern,
    /// The current-state pattern.
    pub state: IrPattern,
    /// The handler's declared effect row.
    #[serde(serialize_with = "serialize_effect_row")]
    pub effect_row: EffectRow,
    /// The body producing the next state.
    pub body: IrExpr,
}

/// A supervisor declaration: a restart strategy, a restart budget, and a list
/// of typed child specs. The effect row is derived, not declared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrSupervisorDef {
    /// The supervisor's name (its own namespace; not a value).
    pub name: String,
    /// The restart strategy: `one_for_one`, `one_for_all`, or `rest_for_one`.
    pub strategy: String,
    /// The maximum number of restarts tolerated within `period` seconds.
    pub intensity: u32,
    /// The restart-intensity window, in seconds.
    pub period: u32,
    /// The child specs, in declaration order.
    pub children: Vec<IrChildSpec>,
    /// The derived per-supervisor effect row: the union of the children's
    /// per-actor effect summaries. Computed, never declared.
    #[serde(serialize_with = "serialize_effect_row")]
    pub effect_row: EffectRow,
    /// The declaration's source position.
    #[serde(skip)]
    pub span: IrSpan,
}

/// One supervised child: an actor started and monitored by the supervisor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrChildSpec {
    /// The child's identifier, unique within the supervisor.
    pub id: String,
    /// The supervised actor's name (resolved in the actor namespace).
    pub actor: String,
    /// The start argument, evaluated during supervisor init. Pure, and typed
    /// against the actor's sole init parameter.
    pub start_args: IrExpr,
    /// The restart disposition: `permanent`, `temporary`, or `transient`.
    pub restart: String,
}

/// A reference to an external (FFI) function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrExternRef {
    /// The function's name.
    pub name: String,
    /// The function's type (a quantified scheme when polymorphic).
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
    /// The backing foreign module, if known. Always `None` in v0.1: the
    /// surface syntax does not yet name an FFI module.
    pub module: Option<String>,
    /// The declaration's source position.
    #[serde(skip)]
    pub span: IrSpan,
}

/// Serializes a list of [`Type`]s as their canonical textual renderings.
fn serialize_types<S>(tys: &[Type], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.collect_seq(tys.iter().map(|ty| format!("{ty}")))
}

// ── expressions ─────────────────────────────────────────────────

/// An expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum IrExpr {
    /// `let name = value in body`
    Let(IrLet),
    /// `λparams → body`
    Lambda(IrLambda),
    /// `func(args)`
    App(IrApp),
    /// `match scrutinee { arms }`
    Match(IrMatch),
    /// `handle { effect → handler, … } in body`
    Handle(IrHandle),
    /// `install { effect → handler, … } in body`
    Install(IrInstall),
    /// `spawn(Actor, args…)`
    Spawn(IrSpawn),
    /// `supervise(SupName)`
    Supervise(IrSupervise),
    /// `stand()`
    Stand(IrStand),
    /// `clock()`
    Clock(IrClock),
    /// `self()`
    SelfRef(IrSelf),
    /// `schedule(clock, pid, msg, delay_ms)`
    Schedule(IrSchedule),
    /// `child(SupName, child_id)`
    Child(IrChild),
    /// `send(pid, msg)`
    Send(IrSend),
    /// `request(pid, ctor[, timeout_ms])`
    Request(IrRequest),
    /// `reply(reply_to, value)`
    Reply(IrReply),
    /// `crash!(message)` / `panic!(message)`
    Crash(IrCrash),
    /// A constructor applied to zero or more arguments.
    Constructor(IrConstructor),
    /// A literal.
    Literal(IrLiteral),
    /// A variable, function, or operator reference.
    Var(IrVar),
    /// A tuple, including unit (`()`).
    Tuple(IrTuple),
    /// A list.
    List(IrList),
    /// A record.
    Record(IrRecord),
    /// A record field access.
    Field(IrField),
}

/// A `let name = value in body` binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrLet {
    /// The bound name.
    pub name: String,
    /// The bound value's type. A polymorphic binding keeps its monomorphic
    /// value type here; each use site carries its own instantiation.
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
    /// The bound value.
    pub value: Box<IrExpr>,
    /// The body the binding is in scope for.
    pub body: Box<IrExpr>,
}

/// A lambda (`λparams → body`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrLambda {
    /// The parameters, in order.
    pub params: Vec<IrParam>,
    /// The body expression.
    pub body: Box<IrExpr>,
    /// The body's (result) type.
    #[serde(serialize_with = "serialize_type")]
    pub body_type: Type,
    /// The effect row of the lambda's own function type. Backends read the
    /// calling convention off it; an open row counts as effectful.
    #[serde(serialize_with = "serialize_effect_row")]
    pub effect_row: EffectRow,
}

/// A function application (`func(args)`). N-ary: `args` is the full argument
/// list, not a curried chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrApp {
    /// The applied function.
    pub func: Box<IrExpr>,
    /// The arguments, in order.
    pub args: Vec<IrExpr>,
    /// The application's result type.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `match scrutinee { arms }` expression. `if`/`then`/`else` desugars to a
/// match over `Bool`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrMatch {
    /// The scrutinee expression.
    pub scrutinee: Box<IrExpr>,
    /// The scrutinee's type.
    #[serde(serialize_with = "serialize_type")]
    pub scrutinee_type: Type,
    /// The arms, in order.
    pub arms: Vec<IrArm>,
    /// The result type (shared by all arm bodies).
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// One arm of a [`IrMatch`]: a pattern and its body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrArm {
    /// The arm's pattern.
    pub pattern: IrPattern,
    /// The arm's body.
    pub body: IrExpr,
}

/// A `handle { effect → handler, … } in body` block: DI-style effect handlers.
///
/// Each arm binds a declared effect to a handler implementation; within the
/// body, the handled effects route to those handlers (parameter threading, when
/// a backend emits it — no resumable continuations). The block's value is its
/// body's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrHandle {
    /// The handler arms, in order.
    pub arms: Vec<IrHandleArm>,
    /// The handled body.
    pub body: Box<IrExpr>,
    /// The block's effect row: the body's effects minus the handled effects,
    /// plus the handlers' own effects.
    #[serde(serialize_with = "serialize_effect_row")]
    pub effect_row: EffectRow,
    /// The block's value type (the body's type).
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// One arm of an [`IrHandle`]: a handled effect and its handler implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrHandleArm {
    /// The handled effect (head and type arguments), e.g. `Log` or
    /// `Tool<ReadRepo>`.
    #[serde(serialize_with = "serialize_effect")]
    pub effect: Effect,
    /// The handler implementation (a function).
    pub handler: IrExpr,
}

/// An `install { effect → handler, … } in body` block: registry-backed
/// default handlers with dynamic extent.
///
/// Each arm binds an effect to a pure handler installed in the runtime's
/// process-independent default registry for the extent of the body (and
/// restored afterwards); spawned actors' tool calls resolve through that
/// registry. Nothing is handled lexically, so the body's effects all remain
/// in the block's row. The block's value is its body's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrInstall {
    /// The handler arms, in order (the arm shape is `handle`'s).
    pub arms: Vec<IrHandleArm>,
    /// The body the handlers are installed for.
    pub body: Box<IrExpr>,
    /// The block's effect row: the body's effects plus `Install`.
    #[serde(serialize_with = "serialize_effect_row")]
    pub effect_row: EffectRow,
    /// The block's value type (the body's type).
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `spawn(Actor, args…)` expression: starts an actor, returning a typed
/// `Pid<Msg>` reference with a `Spawn<Msg>` effect. The actor is a namespace
/// reference, not an expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrSpawn {
    /// The spawned actor's name.
    pub actor: String,
    /// The init arguments, in order.
    pub args: Vec<IrExpr>,
    /// The expression's type: `Pid<Msg>` for the actor's message type.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `supervise(SupName)` expression: starts a declared supervisor's tree,
/// unit-valued with a bare `Supervise` effect. The supervisor is a namespace
/// reference, not an expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrSupervise {
    /// The supervised supervisor's name.
    pub supervisor: String,
    /// The expression's type: unit.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `stand()` expression: blocks until a shutdown signal, then takes the
/// caller's supervision trees down; unit-valued with a bare `Stand` effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrStand {
    /// The expression's type: unit.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `clock()` expression: acquires the runtime clock capability, typed as
/// the built-in opaque `Clock` with a bare `Clock` effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrClock {
    /// The expression's type: `Clock`.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `self()` expression: the enclosing actor's own pid, typed as its
/// `Pid<Msg>`, effect-free.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrSelf {
    /// The expression's type: `Pid<Msg>` for the enclosing actor's message
    /// type.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `schedule(clock, pid, msg, delay_ms)` expression: delivery of a message
/// to a typed `Pid<Msg>` reference after a delay, through a clock capability;
/// unit-valued with a `Schedule<Msg>` effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrSchedule {
    /// The clock capability expression.
    pub clock: Box<IrExpr>,
    /// The destination pid expression.
    pub pid: Box<IrExpr>,
    /// The message expression.
    pub message: Box<IrExpr>,
    /// The delay expression, in milliseconds.
    pub delay: Box<IrExpr>,
    /// The expression's type: unit.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `child(SupName, child_id)` expression: typed lookup of a supervised
/// child's pid, effect-free. Both arguments are namespace references, not
/// expressions; a missing or restarting child crashes rather than returning
/// an option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrChild {
    /// The supervisor's name.
    pub supervisor: String,
    /// The looked-up child's id.
    pub child_id: String,
    /// The expression's type: `Pid<Msg>` for the child actor's message type.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `send(pid, msg)` expression: fire-and-forget delivery to a typed
/// `Pid<Msg>` reference, unit-valued with a `Send<Msg>` effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrSend {
    /// The destination pid expression.
    pub pid: Box<IrExpr>,
    /// The message expression.
    pub message: Box<IrExpr>,
    /// The expression's type: unit.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `request(pid, ctor[, timeout_ms])` expression: builds a message around a
/// fresh `ReplyTo<T>`, sends it, and awaits the reply, with `Send<Msg>` and
/// `Await<T>` effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrRequest {
    /// The destination pid expression.
    pub pid: Box<IrExpr>,
    /// The message-building function (`ReplyTo<T> → Msg`), typically a
    /// message constructor.
    pub message_fn: Box<IrExpr>,
    /// The timeout expression, in milliseconds; `None` for the 5000 ms
    /// default.
    pub timeout: Option<Box<IrExpr>>,
    /// The expression's type: the reply type `T`.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `reply(reply_to, value)` expression: answers a request on its typed
/// reply channel, unit-valued with a `Send<T>` effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrReply {
    /// The reply channel expression.
    pub reply_to: Box<IrExpr>,
    /// The replied value expression.
    pub value: Box<IrExpr>,
    /// The expression's type: unit.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A `crash!(message)` (or `panic!(message)`) expression: divergent process
/// termination.
///
/// It never returns, so it fits any result context; `result_type` is the type
/// demanded at this use site (a fresh variable the checker unified with the
/// surrounding context). Crashing is not an effect — the node carries no row —
/// so it propagates as an Erlang exit the source emitter renders (`erlang:error/1`),
/// caught only by a supervisor. `panic!` is a surface alias and lowers here too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrCrash {
    /// The crash message expression (a `String`).
    pub message: Box<IrExpr>,
    /// The expression's type at this use site.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A constructor applied to zero or more arguments. Operators and `if` lower to
/// [`IrApp`]/[`IrMatch`], but constructors keep their own node so tooling can
/// see the data shape directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrConstructor {
    /// The constructor name.
    pub name: String,
    /// The name of the data type this constructs.
    pub type_name: String,
    /// The arguments, in order. Empty for a nullary constructor.
    pub args: Vec<IrExpr>,
    /// The constructed type.
    #[serde(serialize_with = "serialize_type")]
    pub result_type: Type,
}

/// A literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrLiteral {
    /// The literal value, carrying its source text.
    pub value: LiteralValue,
    /// The literal's type (`Int`, `Float`, or `String`).
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

/// A literal's value, stored as its source text (lossless: BEAM integers are
/// arbitrary-precision, and string text keeps its original escapes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum LiteralValue {
    /// An integer literal, as written.
    Int(Box<str>),
    /// A float literal, as written.
    Float(Box<str>),
    /// A string literal, including its surrounding quotes, as written.
    Str(Box<str>),
}

/// A variable, function, or operator reference.
///
/// Lowered operators (`a + b`) reference a primitive operator by its canonical
/// symbol (`+`, `∧`, …); qualified names (`Mod.member`) reference the imported
/// value by its dotted name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrVar {
    /// The referenced name.
    pub name: String,
    /// The reference's type (instantiated at this use site).
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

/// A tuple, including unit (an empty tuple).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrTuple {
    /// The elements, in order.
    pub elems: Vec<IrExpr>,
    /// The tuple's type.
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

/// A list literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrList {
    /// The elements, in order.
    pub elems: Vec<IrExpr>,
    /// The list's type.
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

/// A record literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrRecord {
    /// The fields, in source order.
    pub fields: Vec<IrRecordField>,
    /// The record's type.
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

/// One field of a record literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrRecordField {
    /// The field label.
    pub label: String,
    /// The field's value.
    pub value: IrExpr,
}

/// A record field access (`receiver.field`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrField {
    /// The receiver expression.
    pub receiver: Box<IrExpr>,
    /// The accessed field label.
    pub field: String,
    /// The field's type.
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

// ── patterns ─────────────────────────────────────────────────────

/// A pattern. Every pattern carries the type of the value it matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum IrPattern {
    /// `Foo(..)` or a nullary `Foo`.
    Constructor(IrConstructorPat),
    /// `(.., ..)`
    Tuple(IrTuplePat),
    /// A literal pattern.
    Literal(IrLiteralPat),
    /// `_`
    Wildcard(IrWildcardPat),
    /// A variable binding.
    Bind(IrBindPat),
}

/// A constructor pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrConstructorPat {
    /// The constructor name.
    pub name: String,
    /// The name of the data type matched.
    pub type_name: String,
    /// The sub-patterns for each field, in order.
    pub fields: Vec<IrPattern>,
    /// The matched type.
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

/// A tuple pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrTuplePat {
    /// The element patterns, in order.
    pub elems: Vec<IrPattern>,
    /// The matched type.
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

/// A literal pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrLiteralPat {
    /// The literal value, carrying its source text.
    pub value: LiteralValue,
    /// The matched type.
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

/// A wildcard pattern (`_`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrWildcardPat {
    /// The matched type.
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

/// A variable binding pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrBindPat {
    /// The bound name.
    pub name: String,
    /// The bound type.
    #[serde(rename = "type", serialize_with = "serialize_type")]
    pub ty: Type,
}

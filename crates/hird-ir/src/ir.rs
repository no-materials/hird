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

// ── module and declarations ─────────────────────────────────────

/// A lowered module: a name and its declarations in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IrModule {
    /// The module's name (path-derived; authoritative).
    pub name: String,
    /// Function, type, and extern declarations, in source order. Imports and
    /// not-yet-elaborated forms (effects, tools, actors, supervisors) are
    /// resolved away or not yet modelled, and do not appear here.
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

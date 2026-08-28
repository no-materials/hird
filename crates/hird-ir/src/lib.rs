// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! IR data structures, lowering, serialization, and pretty-printing.
//!
//! The IR is the stable, fully-typed representation produced after type
//! inference. Every node carries its resolved [`hird_types::Type`]; syntactic
//! sugar is desugared; and the whole tree serializes to JSON for tooling and
//! the MCP server. It is the contract between the compiler frontend and every
//! downstream consumer (codegen, LLM tooling, effect-graph analysis).
//!
//! [`lower_module`] turns a checked module into an [`IrModule`]; [`pretty_print`]
//! renders an [`IrModule`] back to canonical Hirð source. The IR node kinds
//! are re-exported at the crate root. The schema and round-trip property are
//! documented in `docs/ir.md`.
//!
//! # Quick start
//!
//! ```
//! use hird_ast::{AstNode, SourceFile};
//!
//! let parsed = hird_parse::parse("fn answer() = 42", 0);
//! let file = SourceFile::cast(parsed.syntax().clone()).unwrap();
//! let checked = hird_check::check(&file, 0);
//!
//! let module = hird_ir::lower_module(&file, &checked, "Main");
//! assert_eq!(module.declarations.len(), 1);
//! assert!(module.to_json().is_ok());
//!
//! let source = hird_ir::pretty_print(&module);
//! assert!(source.contains("fn answer() \u{2192} Int = 42"));
//! ```

#![no_std]

extern crate alloc;

mod graph;
mod ir;
mod lower;
mod pretty;

pub use graph::{
    ActorNode, ChildNode, ConstructorNode, EFFECT_GRAPH_SCHEMA_VERSION, EffectGraph, EffectRef,
    EffectRowRef, HandlerNode, InitNode, MessageNode, ParamNode, SupervisorNode, ToolNode, TypeRef,
    TypeStructure, effect_graph,
};
pub use hird_types::EffectRow;
pub use ir::{
    IrActorDef, IrActorHandler, IrActorInit, IrApp, IrArm, IrBindPat, IrChild, IrChildSpec,
    IrClock, IrConstructor, IrConstructorDef, IrConstructorPat, IrCrash, IrDecl, IrExpr,
    IrExternRef, IrField, IrFnDef, IrHandle, IrHandleArm, IrInstall, IrLambda, IrLet, IrList,
    IrLiteral, IrLiteralPat, IrMatch, IrModule, IrParam, IrPattern, IrRecord, IrRecordField,
    IrReply, IrRequest, IrSchedule, IrSelf, IrSend, IrSpan, IrSpawn, IrStand, IrSupervise,
    IrSupervisorDef, IrToolDef, IrTuple, IrTuplePat, IrTypeDef, IrVar, IrWildcardPat, LiteralValue,
};
pub use lower::lower_module;
pub use pretty::pretty_print;

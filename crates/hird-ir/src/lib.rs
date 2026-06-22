// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! IR data structures, lowering, and serialization.
//!
//! The IR is the stable, fully-typed representation produced after type
//! inference. Every node carries its resolved [`hird_types::Type`]; syntactic
//! sugar is desugared; and the whole tree serializes to JSON for tooling and
//! the MCP server. It is the contract between the compiler frontend and every
//! downstream consumer (codegen, LLM tooling, effect-graph analysis).
//!
//! [`lower_module`] turns a checked module into an [`IrModule`]; the IR node
//! kinds are in [`mod@ir`]. The schema is documented in `docs/ir.md`.
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
//! ```

#![no_std]

extern crate alloc;

mod ir;
mod lower;

pub use ir::{
    EffectRow, IrApp, IrArm, IrBindPat, IrConstructor, IrConstructorDef, IrConstructorPat, IrDecl,
    IrExpr, IrExternRef, IrField, IrFnDef, IrLambda, IrLet, IrList, IrLiteral, IrLiteralPat,
    IrMatch, IrModule, IrParam, IrPattern, IrRecord, IrRecordField, IrTuple, IrTuplePat, IrTypeDef,
    IrVar, IrWildcardPat, LiteralValue,
};
pub use lower::lower_module;

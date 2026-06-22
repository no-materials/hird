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
//! - `handle { … } in body` lowers to `body`; the handler arms reference
//!   effects, which the IR does not yet model, so until then a handle is its
//!   handled body (exactly what the checker types it as).
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
    AppExpr, AstNode, BinOpExpr, Decl, Expr, ExternDecl, FieldExpr, FnDecl, IfExpr, LambdaExpr,
    LetExpr, Literal, MatchExpr, Pattern, RecordLit, SourceFile, TupleLit, TypeDecl,
};
use hird_check::{CheckedFile, NodeKey};
use hird_parse::SyntaxKind;
use hird_types::Type;

use crate::ir::{
    EffectRow, IrApp, IrArm, IrBindPat, IrConstructor, IrConstructorDef, IrConstructorPat, IrDecl,
    IrExpr, IrExternRef, IrField, IrFnDef, IrLambda, IrLet, IrList, IrLiteral, IrLiteralPat,
    IrMatch, IrModule, IrParam, IrPattern, IrRecord, IrRecordField, IrTuple, IrTuplePat, IrTypeDef,
    IrVar, IrWildcardPat, LiteralValue,
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
    let lowerer = Lowerer { checked };
    let mut declarations = Vec::new();
    for decl in file.declarations() {
        match decl {
            Decl::Fn(d) => declarations.extend(lowerer.lower_fn(&d).map(IrDecl::Fn)),
            Decl::Type(d) => declarations.extend(lowerer.lower_type(&d).map(IrDecl::Type)),
            Decl::Extern(d) => declarations.extend(lowerer.lower_extern(&d).map(IrDecl::Extern)),
            // Imports are resolved away; effects, tools, actors, and
            // supervisors are not yet modelled and carry no IR yet.
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
        Some(IrFnDef {
            name: String::from(name),
            params,
            return_type,
            effect_row: EffectRow::empty(),
            body: self.lower_expr(&body),
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
            Expr::Name(name) => self.lower_name(name.text(), self.expr_type(expr)),
            Expr::Let(le) => self.lower_let(le),
            Expr::Lambda(lambda) => self.lower_lambda(lambda),
            Expr::If(ife) => self.lower_if(ife),
            Expr::Match(me) => self.lower_match(me),
            Expr::Handle(handle) => {
                // Effects are not yet modelled; until then a handle is its body.
                match handle.body() {
                    Some(body) => self.lower_expr(&body),
                    None => self.unit(),
                }
            }
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
        let value = le.value().expect("let has a value");
        let body = le.body().expect("let has a body");
        IrExpr::Let(IrLet {
            name: String::from(le.name().unwrap_or("")),
            ty: self.expr_type(&value),
            value: Box::new(self.lower_expr(&value)),
            body: Box::new(self.lower_expr(&body)),
        })
    }

    /// `λparams → body`. Parameter types come from the lambda's own function
    /// type, so each parameter is explicitly typed.
    fn lower_lambda(&self, lambda: &LambdaExpr) -> IrExpr {
        let (param_tys, body_type) = match self.node_type(lambda.syntax()) {
            Type::TyFn(params, ret) => (params, *ret),
            other => (Vec::new(), other),
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
        Type::TyForall(_, body) => body.as_ref(),
        other => other,
    };
    let (fields, result) = match inner {
        Type::TyFn(fields, result) => (fields.as_slice(), result.as_ref()),
        // A nullary constructor: no fields, the type itself is the result.
        _ => (&[][..], inner),
    };
    let rename = parameter_rename(result, params);
    fields.iter().map(|f| f.substitute(&rename)).collect()
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

---
id: hir-h8qo
status: closed
deps: []
links: []
created: 2026-06-09T06:52:41Z
type: task
priority: 1
assignee: nomaterials
parent: hir-89zs
tags: [phase-3, types, ast]
---
# Complete the hird-ast projection: type expressions and patterns

hird-ast projects declarations and expressions, but type expressions and
patterns are not yet projected (the crate module docs say so outright);
consumers must drop to raw AstNode::syntax. The type checker (hir-lhyh) needs
constructor field types, type parameters, and signature annotations to build
constructor schemes and check function bodies; exhaustiveness (hir-n3si) needs
patterns. This ticket completes the hird-ast typed projection over syntax the
parser already produces.

Project as typed AST nodes, mirroring the existing thin-newtype +
cast/can_cast style:

- Type expressions (CST kinds APP_TYPE, FN_TYPE, TUPLE_TYPE, PAREN_TYPE,
  named/var via IDENT, TYPE_ARGS): a TypeExpr enum with named (name +
  optional type args), function, tuple, and parenthesised/variable forms.
- Patterns (CONSTRUCTOR_PAT, TUPLE_PAT, LITERAL_PAT, WILDCARD_PAT, BIND_PAT):
  a Pattern enum with constructor (name + nested sub-patterns), tuple,
  literal, wildcard, and binding (name) forms; nested patterns reachable.
- Accessors wired onto existing nodes: Param::ty, Constructor::fields (each
  field type), TypeDecl::type_params (parameter names), FnDecl::return_type,
  ExternDecl::return_type, LetExpr::annotation, MatchArm::pattern.

Scope boundary: this is SYNTACTIC projection only. Do not elaborate surface
type syntax into semantic hird_types::Type — that mapping needs the type
environment (type-parameter scope, named-constructor resolution) and lives in
the checker (hir-lhyh). hird-ast must not gain a hird-types dependency. No
inference, no name resolution here.

## Design

Follows the established hird-ast pattern: ast_node! newtypes over a single
SyntaxKind, plus Expr/Decl-style enums with cast_node/cast_element for the
TypeExpr and Pattern sums. Token-backed atomic operands (a bare type variable,
a binding name) follow the Literal/NameRef precedent. Accessors stay lazy over
children; no allocation beyond what the existing projection already does.

## Acceptance Criteria

- TypeExpr enum + AstNode wrappers cover named/applied, function, tuple, and
  parenthesised/variable type forms.
- Pattern enum + wrappers cover constructor, tuple, literal, wildcard, and
  binding patterns, with nested sub-patterns reachable.
- New accessors return the expected structure for representative sources:
  Param::ty, Constructor::fields, TypeDecl::type_params, FnDecl::return_type,
  ExternDecl::return_type, LetExpr::annotation, MatchArm::pattern.
- hird-ast gains no dependency on hird-types; no semantic elaboration or name
  resolution is performed.
- Tests in crates/hird-ast/tests cover each new node and accessor; existing
  tests still pass.
- All public and private items documented (compact-fragment style); cargo fmt
  and cargo clippy -D warnings pass.

---
id: hir-lhyh
status: open
deps: [hir-jj3l]
links: []
created: 2026-05-22T21:37:29Z
type: task
priority: 1
assignee: nomaterials
parent: hir-89zs
tags: [phase-3, types, inference]
---
# Let-polymorphism and ADT type checking

Implement Hindley-Milner type inference with let-polymorphism and algebraic data
type checking.

**Type inference** (Algorithm W variant):
- Traverse the typed AST. For each node, produce a typed version with inferred types.
- Let-bound values: infer the body's type, generalize free type variables, bind
  the generalized scheme. `let id = λx -> x in id(1)` gives id : ∀A. A -> A.
- Lambda-bound values: monomorphic — no generalization inside lambda bodies.
- Function declarations: treat as let-bound (generalize).
- Application: infer function type, infer argument type, unify, return result type.
- Literals: Int, Float, String, Bool have known types.
- If-then-else: condition must be Bool, branches must unify.
- Match expressions: scrutinee type, pattern types, and body types must all be
  consistent (detailed in exhaustiveness ticket).

**ADT type checking**:
- Type declarations create constructors that are typed functions.
  `type Option<A> = Some(A) | None` creates:
  - Some : ∀A. A -> Option<A>
  - None : ∀A. Option<A>
- Recursive types work: `type List<A> = Cons(A, List<A>) | Nil`.
- Type parameters are correctly propagated through constructor applications.
- Constructor usage in expressions is type-checked as function application.

**Environment management**:
- Type environment maps names to type schemes (quantified types).
- Scoping: let and lambda introduce new bindings; match patterns introduce bindings.
- Shadowing: inner bindings shadow outer (warn but don't error).

## Acceptance Criteria

- let id = λx -> x in id(1) infers id : ∀A. A -> A, result : Int.
- Polymorphic let: let f = λx -> x in (f(1), f("hello")) type-checks.
- Monomorphic lambda: λf -> (f(1), f("hello")) is a type error.
- ADT constructors are typed: Some(1) : Option<Int>, None : Option<A>.
- Recursive types: Cons(1, Cons(2, Nil)) : List<Int>.
- If-then-else: branches must unify; non-Bool condition is an error.
- At least 20 snapshot tests covering inference scenarios.
- Property tests: randomly generated well-typed terms infer successfully.


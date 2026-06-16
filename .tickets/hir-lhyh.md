---
id: hir-lhyh
status: closed
deps: [hir-jj3l, hir-h8qo]
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


## Notes

**2026-06-11T14:13:52Z**

Design decisions for implementation. These resolve the points the ticket text
leaves open; each picks the long-term shape over the expedient one.

**D1 — Checker lives in a new crate `hird-check`; `hird-types` stays
syntax-free.** hir-jj3l deliberately reduced hird-types' dependency surface to
hird-lex (Span only); the inference pass needs hird-ast (and transitively the
parser), so it gets its own crate rather than re-entangling the type core.
hird-types keeps the semantic vocabulary (Type, Subst, unify, schemes,
levels); hird-check owns elaboration of surface TypeExpr/Pattern, the
environment, and the inference walker. Phase 5 then extends hird-types with
row *representation* and hird-check with effect *inference* without either
crate learning the other's internals. The epic's "clippy/test pass for
hird-types" AC applies to both crates. Trim hird-types' Cargo description to
"type representation and unification" when this lands.

**D2 — The "typed AST" is a side-table over the CST, not a parallel tree.**
hird-check's output is a checked-file artifact containing: (a) a type table
keyed by CST node identity, i.e. (SyntaxKind, span) — range alone is
ambiguous when a node has a single node child covering its full extent;
(b) the top-level environment, name → scheme; (c) the ADT registry, type
name → constructors with arities and field types (hir-n3si reads this for
exhaustiveness, hir-i0u7 extends it for opaqueness); (d) accumulated
diagnostics. Rationale: the CST remains the single source of truth (spans for
free, nothing to keep in sync), the side-table is the incremental-friendly
shape (recheck = recompute table entries, rust-analyzer-proven), and all
three downstream consumers (n3si, i0u7, Phase 4 IR lowering) are query-shaped.

**D3 — Function types are n-ary; there is no auto-currying.** Change
`TyFn(Box<Type>, Box<Type>)` to `TyFn(Vec<Type>, Box<Type>)` (land this in
hird-types as a small precursor change; jj3l is not sacred). Readings:

- An unparenthesised arrow chain `A → B → C` denotes the 2-ary function
  (A, B) → C. This is already what the parser produces (flat FN_TYPE) and
  what FnType::params/return_type project. A function-returning function is
  written explicitly: `A → (B → C)`.
- fn/extern declarations and lambdas elaborate to n-ary types; `λx y → e` is
  2-ary, not a curried chain.
- Application is checked against the syntactic argument shape. `f(a, b)`
  parses as APP_EXPR(f, TUPLE_LIT) — checked as a 2-ary call. `f()` parses as
  APP_EXPR(f, empty TUPLE_LIT) — a 0-ary call. `f(a)` / bare juxtaposition
  `f a` — a 1-ary call. Passing an actual tuple value is `f((a, b))`
  (APP_EXPR(f, PAREN_EXPR(TUPLE_LIT))) — distinguishable in the CST.
- No implicit partial application in v0.1. If wanted later, add explicit
  capture syntax (Gleam precedent), never currying.

Rationale: BEAM functions are n-ary — a 1:1 arity correspondence keeps Phase
4 codegen direct and inspection honest (ADR-002 makes the generated Erlang a
user artifact). It eliminates the curried-vs-tuple ambiguity instead of
papering over it, and in Phase 5 each function type carries exactly one
effect row in an unambiguous position (TyFn grows a row slot then; do not add
the field now). Promote this to an ADR once implementation validates it.

**D4 — Top-level checking is SCC-ordered; recursion is monomorphic unless
annotated.** Pass 1 over the file registers ADTs + constructor schemes and
the signatures of fully annotated fns (trusted, then checked against their
bodies). Pass 2 builds the top-level reference graph, condenses it to
strongly connected components (small in-crate Tarjan, no new dependency), and
checks SCCs in topological order, generalising after each SCC. Within an
SCC, unannotated members are monomorphic (standard HM monomorphic recursion;
polymorphic recursion only via annotation — undecidable otherwise). Forward
references therefore just work; declaration order is not semantic. Rationale:
order-sensitivity would be a usability bug on day one, and the SCC graph is
the unit the incremental endgame (per-SCC recheck) needs anyway.

**D5 — Generalisation uses Rémy-style levels, not environment scans.**
Subst's Unbound slots gain a level (orthogonal to the union-by-rank rank);
the checker enters/exits a level at each generalisation boundary; bind()
performs level adjustment during its existing occurs walk; generalise
quantifies exactly the variables deeper than the current level. Rationale:
O(1)-amortised vs O(|env|) per let, the OCaml-proven mechanism, and Phase 5
row variables reuse the same machinery unchanged.

**D6 — Bool is a predeclared ADT, not a literal.** The lexer has no
true/false tokens, and a lowercase `true` in pattern position parses as
BIND_PAT — a silent catch-all footgun. Decision: the initial environment is
seeded as if `type Bool = True | False` had been declared; values and
patterns use the PascalCase constructors. if-conditions and comparisons check
against TyCon("Bool"); codegen later lowers True/False to Erlang's true/false
atoms; exhaustiveness over Bool falls out of n3si's standard ADT machinery
with no special-casing. This amends the ticket's "Literals: … Bool" line —
there are Int/Float/String literals only. Add a Bool line to phrasebook.md
when this lands.

**D7 — Operators are monomorphic; no overloading.** `+ - * /` and
`< <= > >=` are (Int, Int) → Int / → Bool, Int-only. Float arithmetic is
deferred to a follow-up lexer/parser ticket introducing distinct operators
(`+.` family, Gleam-style) — do not silently overload. `==` and `!=` are
∀a. (a, a) → Bool (BEAM structural equality is native). `∧ ∨` are
(Bool, Bool) → Bool. Rationale: HM without type classes cannot overload
honestly, and type classes are out of scope possibly forever; predictable
monomorphic operators over hidden constraint machinery.

**D8 — Record field access requires an already-determined record type.**
`e.field` type-checks only when e's type resolves to a concrete TyRecord at
the access site (via annotation or visible construction). Otherwise emit a
dedicated diagnostic: "cannot determine the record type of this expression;
add a type annotation". No guessing with fresh record types. Phase 5 row
polymorphism relaxes this through the extension points the unification
engine already reserved.

**D9 — Error policy: per-declaration isolation, multi-error per file.** A
type error inside one body aborts that body's check; the declaration keeps
its annotated or seeded signature so downstream SCCs still check, and
diagnostics accumulate across the file. No poison/error type in v0.1 —
revisit only with evidence it's needed. Effect annotations (`! { … }`) in
signatures are parsed but wholly ignored by this ticket; Phase 5 owns them.

Implementation note: the property-test AC ("randomly generated well-typed
terms infer successfully") requires a type-directed term generator — the
largest single test-infra item here. It lives in hird-check's tests
(proptest, matching the hird-lex/hird-parse convention).

**2026-06-11T15:00:25Z**

Implemented per D1-D9 above.

hird-types: TyFn is now n-ary (`TyFn(Vec<Type>, Box<Type>)`) with matching
display (flat arrow chains denote arity; nested functions parenthesise on
both sides; 0-ary renders `() → T`); Subst gained Rémy levels
(enter/exit_level, level-adjusting occurs walk on bind, min-level union),
generalize/instantiate, and Type::normalized for display-canonical var
naming. 46 unit tests.

hird-check (new crate): elaboration of surface TypeExpr with
closed/fresh/skolem variable modes, ADT registry with seeded
`type Bool = True | False`, scoped env with shadow warnings, full
expression/pattern inference walker, Tarjan-SCC dependency-ordered
top-level checking with per-declaration error isolation, and a CheckedFile
artifact: (SyntaxKind, span)-keyed type side-table, resolved top-level
bindings, ADT constructor table, source-ordered diagnostics (codes
C0001-C0014). 45 insta snapshot tests + a type-directed proptest generator
(well-typed terms infer their target type, 64 cases).

hird-ast: added token-backed `syntax()` accessors on Literal/NameRef/
NameType (spans for atomic operands).

Notes for review:
- The grammar already mandates `name: Type` on fn parameters, so only
  return types are ever inferred at top level; type-variable annotations
  (`fn pair(x: a, y: b)`) are the way to write polymorphic unannotated-
  return functions. The checker handles unannotated params anyway should
  the grammar relax.
- Annotated-signature rigidity is enforced by skolemising signature vars
  as lowercase TyCons (unutterable as user types), giving "expected `a`,
  got `Int`" mismatches with no extra machinery (C0012 reserved code was
  not needed for this; it is used for unbound type parameters instead).
- A declaration that fails its body check keeps its annotated/placeholder
  scheme (D9), which surfaces as `name : ∀a. a` in bindings for aborted
  inferred declarations - deterministic and transparent in snapshots.

fmt, clippy (-D warnings), and full workspace tests pass.

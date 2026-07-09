# The Error Model

Hirð draws a single, compiler-enforced line between two kinds of failure:

- **Domain errors** are values. A function that can fail with one carries an
  `Exn<E>` entry in its effect row; the caller handles it with ordinary
  control flow, and the process keeps running.
- **Crashes** are process death. `crash!(msg)` (and its alias `panic!`)
  terminates the process; it propagates as an Erlang exit and is caught only
  by a supervisor, never by ordinary Hirð code.

This document explains why the line exists, where each side lives in the type
system, and how to choose between them.

## Why two kinds of failure

OTP's guiding advice is "let it crash": a process that hits an unrecoverable
situation should die and let its supervisor restart it from a known-good
state, rather than thread defensive error handling through every call.
Effect rows pull the other way: they make a function's failure modes part of
its type, so callers must acknowledge them.

Both are right, for different failures. A malformed LLM response is a normal,
expected outcome the caller can retry or route around — it should be a value.
A dropped database connection mid-transaction is not something the caller can
meaningfully repair — the honest response is to die and restart clean. A
language that models only one of these forces the other into an awkward shape:
either every function grows a `Crash` case it cannot handle, or genuinely
recoverable errors masquerade as fatal ones.

So Hirð keeps both and makes the choice visible in types.

## Domain errors: values in the effect row

A recoverable failure is an `Exn<E>` effect, where `E` is the error type. It
appears in the row exactly like any other effect:

```
effect Exn<t>

fn parse_config(raw: String) → Config ! {Exn<ParseError>} = ...
```

The row is a checked, exhaustive list of the domain errors a function can
produce. A caller sees `Exn<ParseError>` in the signature and must account for
it — by handling it, or by letting it flow into its own row. `Exn`-carrying
code never terminates the process on its own; the error is data that travels
through normal returns and pattern matches.

Because a domain error is a value, the ordinary tools apply. An
`Option`/`Result`-style result is matched:

```
fn load(raw: String) → Config = match try_parse(raw) {
  Ok(config)  → config,
  Err(reason) → default_config(),
}
```

and an effect handler can intercept the `Exn` effect to mock, log, or recover
it without changing the code under test.

A function whose row contains **no** `Exn` entry cannot produce a domain error.
Barring bugs, resource exhaustion, and explicit crashes, it runs to
completion. The empty-of-`Exn` row is a real guarantee, not a hint.

## Crashes: divergence outside the row

Some situations are not worth recovering from in place: an invariant that
should never be violated, a resource that vanished, a case the code is not
built to continue past. For these, `crash!` crosses from value-space into
crash-space:

```
fn must_get(key: Key, table: Config) → Value = match lookup(key, table) {
  Some(value) → value,
  None        → crash!("config missing required key"),
}
```

`crash!(msg)` takes a single `String` and **never returns**. It terminates the
current process with that message, propagating as an Erlang exit. It is caught
by the process's supervisor, which applies its restart strategy — not by any
`match`, handler, or caller in between. `panic!` is an exact alias; use
whichever name reads better.

Three consequences of "never returns" shape how `crash!` types and composes:

1. **It fits any result position.** In `must_get` above, the `None` arm must
   produce a `Value` to match the `Some` arm, and `crash!` obliges — not by
   producing a `Value`, but because a value that never arrives cannot conflict
   with any type. `crash!` is typed as `∀a. (String) → a`: each use picks up
   whatever type the context demands, the same way `identity : ∀a. a → a`
   adapts to its call site. No annotation is ever needed.

2. **It is not in the effect row.** The possibility of crashing is deliberately
   *not* represented as an effect. A `Crash` effect would appear on nearly
   every function that does I/O or calls a crashing helper, so it would carry
   no discriminating information — the opposite of what the row is for. The row
   stays the exhaustive list of *domain* errors; crashing is orthogonal to it.
   `crash!` itself carries the empty row.

3. **It cannot be caught in Hirð.** There is no `try`/`catch` for crashes.
   Crossing into crash-space is a one-way door out of the current process;
   recovery is the supervisor's job, expressed as a restart strategy rather
   than inline code.

## How the two interact with supervision

A supervised actor sits at the boundary. Inside its handlers, domain errors
are values it can inspect and answer — a `ParseError` from a tool call becomes
a reply to the requester, and the actor lives on. A crash is the actor
conceding that it cannot continue: it exits, and its supervisor restarts it
according to the declared strategy, intensity, and period.

This is why the distinction is worth enforcing. The supervisor's restart
budget is a statement about crashes — "restart this child up to N times in T
seconds, then give up." If recoverable errors also reached the supervisor as
exits, that budget would be spent on failures the actor could have handled
itself, and a burst of ordinary bad input could tear down a healthy tree.
Keeping domain errors as values means only genuine, unrecoverable failures
count against the budget.

Resource-level failures that Hirð does not model as values crash the same way
`crash!` does: out-of-memory, a severed connection, a `request` whose fixed
timeout expires. They reach the supervisor as exits, indistinguishable from an
explicit `crash!`, and are handled by the same restart machinery.

## Choosing between them

Use a **domain error** (`Exn<E>`) when:

- the failure is expected in normal operation (bad input, a miss, a rejected
  request);
- the caller can plausibly do something useful with it (retry, fall back,
  report);
- you want the failure mode visible in the signature and checked at call
  sites.

Use a **crash** (`crash!` / `panic!`) when:

- the situation is a violated invariant or a genuinely unrecoverable state;
- there is no sensible local recovery, and restarting from a clean state is
  the right response;
- threading an error value up would only add noise no caller would act on.

When in doubt, prefer the domain error: it is additive to make a caller handle
a value, but turning a crash back into a value after callers have come to rely
on "it never returns" is a breaking change.

## What the compiler does and does not check

The checker enforces:

- `crash!`'s argument is a `String`.
- A crash expression adopts its context's type, so it composes in any position
  without annotation.
- A crash contributes nothing to the effect row; a function's row remains the
  exhaustive list of its domain errors.

The checker does **not** track divergence. It cannot warn that a function which
handles every `Exn` in its row might still crash by calling a crashing helper,
because "this expression never returns" is not information the type system
carries. Divergence-aware diagnostics — dead code after a `crash!`, or
exhaustiveness that credits a diverging arm — are deliberately out of scope;
the value-typed encoding of `crash!` accepts exactly the same programs a
dedicated bottom type would, so adding that precision later changes no existing
program's meaning.

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

use std::fmt::Write;

use hird_ast::{AstNode, SourceFile};
use hird_check::Severity;

/// Parses, checks, and renders `source` as resolved top-level bindings and
/// diagnostics.
fn check_str(source: &str) -> String {
    let parsed = hird_parse::parse(source, 0);
    assert!(
        parsed.is_ok(),
        "test source has parse errors: {:?}",
        parsed.diagnostics()
    );
    let file = SourceFile::cast(parsed.syntax().clone()).expect("root is a source file");
    let checked = hird_check::check(&file, 0);
    let mut out = String::new();
    for (name, ty) in &checked.bindings {
        writeln!(out, "{name} : {}", ty.normalized()).unwrap();
    }
    for diag in &checked.diagnostics {
        let severity = match diag.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        writeln!(
            out,
            "{severity}[{:?}] {}..{}: {}",
            diag.code, diag.span.start, diag.span.end, diag.message
        )
        .unwrap();
    }
    out
}

/// A well-formed counter actor whose handlers exercise tool effects, shared
/// by the positive tests.
const COUNTER: &str = "\
effect Tool<t>
effect Spawn<t>
type Path = Path(String)
type St = St(Int)
tool ReadRepo : { path: Path } -> St
actor Planner {
  state: St,
  message: PlannerMsg = | Plan(Path) | Stop,
  init: fn(start: St) -> St ! {} = start,
  handle Plan(p), st -> St ! {Tool<ReadRepo>} = read_repo({ path: p }),
  handle Stop, st -> St ! {} = st,
} ! {Tool<ReadRepo>}
";

// ── declarations ────────────────────────────────────────────────

/// An actor declaration registers its message type as an ordinary ADT: the
/// constructors are value bindings any sender can use, while the actor name
/// itself binds nothing in the value namespace.
#[test]
fn actor_declares_typed_mailbox() {
    insta::assert_snapshot!(check_str(COUNTER));
}

/// A `ReplyTo<T>` payload in a message constructor type-checks: `ReplyTo` is
/// a built-in type constructor like `List`.
#[test]
fn reply_to_in_message_constructor() {
    insta::assert_snapshot!(check_str(
        "type Status = Status(Int)\n\
         type St = St(Int)\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Get(ReplyTo<Status>) | Stop,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Get(reply_to), st -> St ! {} = st,\n\
           handle Stop, st -> St ! {} = st,\n\
         }"
    ));
}

/// The message type is a first-class ADT: external code matches over it with
/// the usual exhaustiveness checking.
#[test]
fn message_type_is_ordinary_adt() {
    insta::assert_snapshot!(check_str(
        "type St = St(Int)\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Inc | Dec,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Inc, st -> St ! {} = st,\n\
           handle Dec, st -> St ! {} = st,\n\
         }\n\
         fn describe(m: Msg) -> Int = match m { Inc -> 1 }"
    ));
}

// ── spawn ───────────────────────────────────────────────────────

/// `spawn` returns `Pid<Msg>` with a `Spawn<Msg>` effect, checked against the
/// caller's declared row.
#[test]
fn spawn_returns_typed_pid() {
    let source = format!(
        "{COUNTER}\n\
         fn boot(s: St) -> Pid<PlannerMsg> ! {{Spawn<PlannerMsg>}} = spawn(Planner, s)"
    );
    insta::assert_snapshot!(check_str(&source));
}

/// A caller that omits `Spawn<Msg>` from its declared row is rejected.
#[test]
fn spawn_effect_must_be_declared() {
    let source = format!(
        "{COUNTER}\n\
         fn boot(s: St) -> Pid<PlannerMsg> = spawn(Planner, s)"
    );
    insta::assert_snapshot!(check_str(&source));
}

/// Spawning an undeclared actor is an error in the actor namespace.
#[test]
fn spawn_unknown_actor_rejected() {
    insta::assert_snapshot!(check_str("fn boot() = spawn(Ghost)"));
}

/// Spawn arguments are checked against the actor's init parameters: count…
#[test]
fn spawn_wrong_arity_rejected() {
    let source = format!("{COUNTER}\nfn boot() = spawn(Planner)");
    insta::assert_snapshot!(check_str(&source));
}

/// …and type.
#[test]
fn spawn_arg_type_mismatch_rejected() {
    let source = format!("{COUNTER}\nfn boot() = spawn(Planner, 42)");
    insta::assert_snapshot!(check_str(&source));
}

// ── state encapsulation ─────────────────────────────────────────

/// An actor's name is not a value: state and members are unreachable from
/// outside the handlers.
#[test]
fn actor_state_is_encapsulated() {
    let source = format!("{COUNTER}\nfn peek() = Planner.state");
    insta::assert_snapshot!(check_str(&source));
}

/// Referencing the bare actor name as a value is the same violation.
#[test]
fn actor_name_is_not_a_value() {
    let source = format!("{COUNTER}\nfn grab() = Planner");
    insta::assert_snapshot!(check_str(&source));
}

// ── effect summaries ────────────────────────────────────────────

/// An actor whose declared summary omits a handler's effect is rejected.
#[test]
fn effect_summary_mismatch_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         type Path = Path(String)\n\
         type St = St(Int)\n\
         tool ReadRepo : { path: Path } -> St\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Plan(Path),\n\
           handle Plan(p), st -> St ! {Tool<ReadRepo>} = read_repo({ path: p }),\n\
           init: fn(s: St) -> St ! {} = s,\n\
         }"
    ));
}

/// A summary declaring effects no member performs is rejected too: the
/// summary is checked for equality, not containment.
#[test]
fn effect_summary_overdeclaration_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         type Path = Path(String)\n\
         type St = St(Int)\n\
         tool ReadRepo : { path: Path } -> St\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Stop,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Stop, st -> St ! {} = st,\n\
         } ! {Tool<ReadRepo>}"
    ));
}

/// A handler whose body performs effects its declared row omits is rejected,
/// exactly as a function body is.
#[test]
fn handler_row_mismatch_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         type Path = Path(String)\n\
         type St = St(Int)\n\
         tool ReadRepo : { path: Path } -> St\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Plan(Path),\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Plan(p), st -> St ! {} = read_repo({ path: p }),\n\
         }"
    ));
}

/// The init body's effects are checked against init's declared row.
#[test]
fn init_row_mismatch_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         type Path = Path(String)\n\
         type St = St(Int)\n\
         tool Boot : { path: Path } -> St\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Stop,\n\
           init: fn(p: Path) -> St ! {} = boot({ path: p }),\n\
           handle Stop, st -> St ! {} = st,\n\
         }"
    ));
}

// ── handlers ────────────────────────────────────────────────────

/// Two handlers for the same constructor are rejected; the second reports
/// against the first.
#[test]
fn duplicate_handler_rejected() {
    insta::assert_snapshot!(check_str(
        "type St = St(Int)\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Stop,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Stop, st -> St ! {} = st,\n\
           handle Stop, st -> St ! {} = st,\n\
         }"
    ));
}

/// A handler naming a constructor the message type does not declare is
/// rejected.
#[test]
fn handler_unknown_constructor_rejected() {
    insta::assert_snapshot!(check_str(
        "type St = St(Int)\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Stop,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Nope, st -> St ! {} = st,\n\
         }"
    ));
}

/// A handler naming a constructor of a different type gets a tailored
/// diagnostic naming the message type.
#[test]
fn handler_foreign_constructor_rejected() {
    insta::assert_snapshot!(check_str(
        "type St = St(Int)\n\
         type Other = Whoops\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Stop,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Whoops, st -> St ! {} = st,\n\
         }"
    ));
}

/// A handler body must produce the state type: the next state.
#[test]
fn handler_body_must_return_state() {
    insta::assert_snapshot!(check_str(
        "type St = St(Int)\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Stop,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Stop, st -> St ! {} = 42,\n\
         }"
    ));
}

/// The state pattern binds the current state at the declared state type;
/// destructuring it in the pattern works like any pattern.
#[test]
fn state_pattern_binds_state() {
    insta::assert_snapshot!(check_str(
        "type St = St(Int)\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Inc,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Inc, St(n) -> St ! {} = St(n + 1),\n\
         }"
    ));
}

// ── structure errors ────────────────────────────────────────────

/// A missing `init` member is a structure error.
#[test]
fn missing_init_rejected() {
    insta::assert_snapshot!(check_str(
        "type St = St(Int)\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Stop,\n\
           handle Stop, st -> St ! {} = st,\n\
         }"
    ));
}

/// An unknown member name is a structure error.
#[test]
fn unknown_member_rejected() {
    insta::assert_snapshot!(check_str(
        "type St = St(Int)\n\
         actor A {\n\
           state: St,\n\
           mailbox: St,\n\
           message: Msg = | Stop,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Stop, st -> St ! {} = st,\n\
         }"
    ));
}

/// Two actors sharing a name collide in the actor namespace.
#[test]
fn duplicate_actor_rejected() {
    insta::assert_snapshot!(check_str(
        "type St = St(Int)\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Stop,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Stop, st -> St ! {} = st,\n\
         }\n\
         actor A {\n\
           state: St,\n\
           message: Msg2 = | Halt,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Halt, st -> St ! {} = st,\n\
         }"
    ));
}

// ── messaging ───────────────────────────────────────────────────

/// A counter actor with a request/reply protocol, shared by the messaging
/// tests: `Get` carries a reply channel its handler answers with `reply`.
const MESSAGING: &str = "\
effect Send<t>
effect Await<t>
type Status = Status(Int)
type St = St(Int)
actor Counter {
  state: St,
  message: Msg = | Inc | Get(ReplyTo<Status>),
  init: fn(s: St) -> St ! {} = s,
  handle Inc, St(n) -> St ! {} = St(n + 1),
  handle Get(r), St(n) -> St ! {Send<Status>} = let sent = reply(r, Status(n)) in St(n),
} ! {Send<Status>}
";

/// A complete handler set with a `reply` in a handler body type-checks:
/// `reply` contributes plain `Send<T>` to the handler's row and the summary.
#[test]
fn reply_in_handler_checks() {
    insta::assert_snapshot!(check_str(MESSAGING));
}

/// `send` types against the pid's message type, is unit-valued, and has a
/// `Send<Msg>` effect checked against the caller's declared row.
#[test]
fn send_typed_against_pid() {
    let source = format!("{MESSAGING}\nfn poke(p: Pid<Msg>) ! {{Send<Msg>}} = send(p, Inc)");
    insta::assert_snapshot!(check_str(&source));
}

/// A message of the wrong type for the pid is rejected.
#[test]
fn send_message_type_mismatch_rejected() {
    let source = format!("{MESSAGING}\nfn poke(p: Pid<Msg>) ! {{Send<Msg>}} = send(p, Status(1))");
    insta::assert_snapshot!(check_str(&source));
}

/// A `send` destination must be a `Pid`; a `ReplyTo` is not one — `reply` is
/// the only operation on a reply channel.
#[test]
fn send_to_reply_channel_rejected() {
    let source = format!(
        "{MESSAGING}\nfn answer(r: ReplyTo<Status>) ! {{Send<Status>}} = send(r, Status(1))"
    );
    insta::assert_snapshot!(check_str(&source));
}

/// A caller whose declared row omits `Send<Msg>` is rejected.
#[test]
fn send_effect_must_be_declared() {
    let source = format!("{MESSAGING}\nfn poke(p: Pid<Msg>) = send(p, Inc)");
    insta::assert_snapshot!(check_str(&source));
}

/// `request` types the reply channel against the constructor and returns the
/// reply type, with distinct `Send<Msg>` and `Await<T>` effects.
#[test]
fn request_returns_reply_type() {
    let source = format!(
        "{MESSAGING}\n\
         fn query(p: Pid<Msg>) -> Status ! {{Send<Msg>, Await<Status>}} = request(p, Get)"
    );
    insta::assert_snapshot!(check_str(&source));
}

/// The message builder must take a reply channel: a nullary constructor of
/// the right message type is still rejected.
#[test]
fn request_builder_must_take_reply_channel() {
    let source = format!(
        "{MESSAGING}\n\
         fn query(p: Pid<Msg>) -> Status ! {{Send<Msg>, Await<Status>}} = request(p, Inc)"
    );
    insta::assert_snapshot!(check_str(&source));
}

/// A caller whose declared row omits the `Await<T>` half of a request is
/// rejected: the send and the blocking wait are separate effects.
#[test]
fn request_await_effect_must_be_declared() {
    let source =
        format!("{MESSAGING}\nfn query(p: Pid<Msg>) -> Status ! {{Send<Msg>}} = request(p, Get)");
    insta::assert_snapshot!(check_str(&source));
}

/// `reply` outside a handler works anywhere a `ReplyTo<T>` is in hand.
#[test]
fn reply_typed_against_channel() {
    let source = format!(
        "{MESSAGING}\nfn answer(r: ReplyTo<Status>) ! {{Send<Status>}} = reply(r, Status(1))"
    );
    insta::assert_snapshot!(check_str(&source));
}

/// A replied value must match the channel's type parameter.
#[test]
fn reply_value_type_mismatch_rejected() {
    let source =
        format!("{MESSAGING}\nfn answer(r: ReplyTo<Status>) ! {{Send<St>}} = reply(r, St(1))");
    insta::assert_snapshot!(check_str(&source));
}

// ── receive exhaustiveness ──────────────────────────────────────

/// An actor missing a handler is rejected with the unhandled variant named.
#[test]
fn missing_handler_rejected() {
    insta::assert_snapshot!(check_str(
        "type St = St(Int)\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Inc | Stop,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Inc, st -> St ! {} = st,\n\
         }"
    ));
}

/// Several unhandled variants are all listed, in declaration order.
#[test]
fn missing_handlers_all_listed() {
    insta::assert_snapshot!(check_str(
        "type Status = Status(Int)\n\
         type St = St(Int)\n\
         actor A {\n\
           state: St,\n\
           message: Msg = | Inc | Get(ReplyTo<Status>) | Stop,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Inc, st -> St ! {} = st,\n\
         }"
    ));
}

// ── composition ─────────────────────────────────────────────────

/// Message payloads may carry other actors' typed references: message
/// headers are registered before any constructor field elaborates.
#[test]
fn message_can_carry_foreign_pid() {
    insta::assert_snapshot!(check_str(
        "type St = St(Int)\n\
         actor Worker {\n\
           state: St,\n\
           message: WorkerMsg = | Run,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Run, st -> St ! {} = st,\n\
         }\n\
         actor Boss {\n\
           state: St,\n\
           message: BossMsg = | Register(Pid<WorkerMsg>),\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Register(worker), st -> St ! {} = st,\n\
         }"
    ));
}

/// A handler may spawn: the `Spawn` effect joins the handler's row and the
/// actor's summary like any other effect.
#[test]
fn handler_can_spawn() {
    insta::assert_snapshot!(check_str(
        "effect Spawn<t>\n\
         type St = St(Int)\n\
         actor Worker {\n\
           state: St,\n\
           message: WorkerMsg = | Run,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Run, st -> St ! {} = st,\n\
         }\n\
         actor Boss {\n\
           state: St,\n\
           message: BossMsg = | Hire,\n\
           init: fn(s: St) -> St ! {} = s,\n\
           handle Hire, st -> St ! {Spawn<WorkerMsg>} =\n\
             let worker = spawn(Worker, st) in st,\n\
         } ! {Spawn<WorkerMsg>}"
    ));
}

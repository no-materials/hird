// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

use std::fmt::Write;

use hird_ast::{AstNode, Decl, SourceFile};
use hird_check::{NodeKey, Severity};

/// Parses and checks `source`, rendering its diagnostics followed by each
/// supervisor's derived effect row (the union of its children's per-actor
/// summaries) — the two facets supervisor checking produces.
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
    for decl in file.declarations() {
        if let Decl::Supervisor(sup) = decl
            && let Some(row) = checked.effect_row_at(NodeKey::of_node(sup.syntax()))
        {
            writeln!(out, "supervisor {} ! {row}", sup.name().unwrap_or("?")).unwrap();
        }
    }
    out
}

/// Two actors with distinct tool-effect summaries, a many-parameter actor that
/// cannot be supervised, and pure/effectful config helpers — the surface the
/// supervisor tests declare children over.
const PRELUDE: &str = "\
effect Tool<t>
type Path = Path(String)
type Title = Title(String)
type St = St(Int)
tool ReadRepo : { path: Path } -> St
tool CreateTicket : { title: Title } -> St
fn planner_config() -> St = St(0)
fn worker_config() -> St = St(1)
fn read_config() -> St ! {Tool<ReadRepo>} = read_repo({ path: Path(\"repo\") })
actor Planner {
  state: St,
  message: PlannerMsg = | Plan(Path) | Stop,
  init: fn(c: St) -> St ! {} = c,
  handle Plan(p), st -> St ! {Tool<ReadRepo>} = read_repo({ path: p }),
  handle Stop, st -> St ! {} = st,
} ! {Tool<ReadRepo>}
actor Worker {
  state: St,
  message: WorkerMsg = | Work(Title) | Halt,
  init: fn(c: St) -> St ! {} = c,
  handle Work(t), st -> St ! {Tool<CreateTicket>} = create_ticket({ title: t }),
  handle Halt, st -> St ! {} = st,
} ! {Tool<CreateTicket>}
actor Pair {
  state: St,
  message: PairMsg = | Ping,
  init: fn(a: St, b: St) -> St ! {} = a,
  handle Ping, st -> St ! {} = st,
}
";

/// Appends `sup` to the shared prelude.
fn with_prelude(sup: &str) -> String {
    format!("{PRELUDE}{sup}")
}

// ── valid declarations ───────────────────────────────────────────

/// A well-formed supervisor type-checks with no diagnostics; its derived effect
/// row is the child actor's per-actor summary.
#[test]
fn valid_supervisor() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor PlannerSup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: planner, actor: Planner, start_args: planner_config(), restart: permanent },
  ]
}"
    )));
}

/// The derived effect row is the union of every child actor's per-actor
/// summary, computed automatically rather than declared.
#[test]
fn supervisor_effect_summary_unions_children() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor RootSup {
  strategy: one_for_one,
  intensity: 3,
  period: 10,
  children: [
    { id: planner, actor: Planner, start_args: planner_config(), restart: permanent },
    { id: worker, actor: Worker, start_args: worker_config(), restart: transient },
  ]
}"
    )));
}

/// A supervisor may declare zero children; its derived row is then empty.
#[test]
fn empty_children_permitted() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Idle {
  strategy: one_for_one,
  intensity: 1,
  period: 5,
  children: []
}"
    )));
}

// ── child-spec validation ────────────────────────────────────────

/// A child referencing an actor no declaration introduces is a compile error.
#[test]
fn unresolved_child_actor() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: ghost, actor: Ghost, start_args: planner_config(), restart: permanent },
  ]
}"
    )));
}

/// A `start_args` whose type does not match the child actor's sole init
/// parameter is a type error.
#[test]
fn start_args_type_mismatch() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: planner, actor: Planner, start_args: Path(\"x\"), restart: permanent },
  ]
}"
    )));
}

/// `start_args` is evaluated during supervisor init and must be pure; an
/// effectful expression is rejected.
#[test]
fn effectful_start_args() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: planner, actor: Planner, start_args: read_config(), restart: permanent },
  ]
}"
    )));
}

/// A supervised actor's init must take exactly one parameter; a many-parameter
/// init cannot be supervised.
#[test]
fn init_arity_must_be_one() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: pair, actor: Pair, start_args: planner_config(), restart: permanent },
  ]
}"
    )));
}

/// Two children sharing an id within one supervisor is a compile error.
#[test]
fn duplicate_child_id() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: c, actor: Planner, start_args: planner_config(), restart: permanent },
    { id: c, actor: Worker, start_args: worker_config(), restart: transient },
  ]
}"
    )));
}

/// An invalid `restart` disposition is rejected.
#[test]
fn invalid_restart() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: planner, actor: Planner, start_args: planner_config(), restart: forever },
  ]
}"
    )));
}

/// An unknown or missing child-spec field is a compile error.
#[test]
fn malformed_child_spec() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: planner, actor: Planner, restart: permanent, bogus: 1 },
  ]
}"
    )));
}

// ── body-field validation ────────────────────────────────────────

/// The body fields are a closed set declared at most once; unknown and
/// duplicate fields are compile errors.
#[test]
fn unknown_and_duplicate_fields() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_one,
  intensity: 5,
  intensity: 6,
  period: 60,
  bogus: 1,
  children: []
}"
    )));
}

/// A missing required field is a compile error.
#[test]
fn missing_required_field() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_one,
  intensity: 5,
  children: []
}"
    )));
}

/// `intensity` and `period` must be positive integers.
#[test]
fn intensity_and_period_positive() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_one,
  intensity: 0,
  period: sixty,
  children: []
}"
    )));
}

// ── restart strategy ─────────────────────────────────────────────

/// `one_for_all` and `rest_for_one` parse and type-check but warn as
/// unimplemented in v0.1; only `one_for_one` is supported.
#[test]
fn unsupported_strategy_warns() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_all,
  intensity: 5,
  period: 60,
  children: [
    { id: planner, actor: Planner, start_args: planner_config(), restart: permanent },
  ]
}"
    )));
}

/// An unknown strategy is a compile error, distinct from the unimplemented
/// warning.
#[test]
fn unknown_strategy_rejected() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: all_for_one,
  intensity: 5,
  period: 60,
  children: []
}"
    )));
}

// ── supervisor namespace ─────────────────────────────────────────

/// Two supervisors sharing a name collide in the supervisor namespace.
#[test]
fn duplicate_supervisor_name() {
    insta::assert_snapshot!(check_str(&with_prelude(
        "supervisor Sup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: []
}
supervisor Sup {
  strategy: one_for_one,
  intensity: 1,
  period: 1,
  children: []
}"
    )));
}

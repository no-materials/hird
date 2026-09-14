// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Baseline diffing over real lowered programs: widening, narrowing, and
//! neutral changes are classified per the identity contract, and the report
//! round-trips through JSON.

use hird_ast::{AstNode, SourceFile};
use hird_ir::{ProgramGraph, effect_graph, lower_module};
use hird_policy::{DIFF_SCHEMA_VERSION, Detail, GraphDiff, NodeKind, Severity, diff};

/// The baseline: a tool, an actor using it, and a supervisor.
const PLANNER: &str = "type Path = Path(String)\n\
     type St = St(Int)\n\
     tool ReadRepo : { path: Path } -> Int\n\
     fn read(p: Path) -> Int ! {Tool<ReadRepo>} = read_repo({ path: p })\n\
     fn default_config() -> St = St(0)\n\
     actor Planner {\n\
       state: St,\n\
       message: PlannerMsg = | PlanRepo(Path) | Shutdown,\n\
       init: fn(c: St) ! {} = c,\n\
       handle PlanRepo(p), st ! {Tool<ReadRepo>} = match read(p) { n -> Continue(St(n)) },\n\
       handle Shutdown, st ! {} = Stop,\n\
     } ! {Tool<ReadRepo>}\n\
     supervisor PlannerSup {\n\
       strategy: one_for_one,\n\
       intensity: 5,\n\
       period: 60,\n\
       children: [\n\
         { id: planner, actor: Planner, start_args: default_config(), restart: permanent },\n\
       ]\n\
     }";

/// Parses, checks, and lowers `source` as module `name`.
fn graph(source: &str, name: &str) -> ProgramGraph {
    let parsed = hird_parse::parse(source, 0);
    assert!(
        parsed.is_ok(),
        "test source has parse errors: {:?}",
        parsed.diagnostics()
    );
    let file = SourceFile::cast(parsed.syntax().clone()).expect("root is a source file");
    let checked = hird_check::check(&file, 0);
    assert!(
        !checked.has_errors(),
        "test source has type errors: {:?}",
        checked.diagnostics
    );
    ProgramGraph::new([effect_graph(&lower_module(&file, &checked, name))])
}

/// `PLANNER` with `needle` replaced by `replacement`, which must occur.
fn edited(needle: &str, replacement: &str) -> String {
    assert!(PLANNER.contains(needle), "fixture lacks `{needle}`");
    PLANNER.replace(needle, replacement)
}

#[test]
fn identical_programs_have_no_changes() {
    let base = graph(PLANNER, "Planner");
    let report = diff(&base, &base);
    assert_eq!(report.schema_version, DIFF_SCHEMA_VERSION);
    assert!(report.changes.is_empty(), "{:?}", report.changes);
    assert!(!report.widens);
}

#[test]
fn line_churn_is_not_a_change() {
    let base = graph(PLANNER, "Planner");
    let shifted = format!("// a leading comment\n\n\n{PLANNER}");
    let report = diff(&base, &graph(&shifted, "Planner"));
    assert!(report.changes.is_empty(), "{:?}", report.changes);
}

#[test]
fn a_new_tool_in_a_handler_row_widens() {
    let base = graph(PLANNER, "Planner");
    let widened = edited(
        "handle Shutdown, st ! {} = Stop,",
        "handle Shutdown, st ! {Tool<Probe>} = match probe({ note: \"bye\" }) { _ -> Stop },",
    )
    .replace("} ! {Tool<ReadRepo>}", "} ! {Tool<ReadRepo>, Tool<Probe>}")
    .replace(
        "tool ReadRepo : { path: Path } -> Int\n",
        "tool ReadRepo : { path: Path } -> Int\ntool Probe : { note: String } -> ()\n",
    );
    let report = diff(&base, &graph(&widened, "Planner"));
    assert!(report.widens);

    let handler = report
        .changes
        .iter()
        .find(|c| matches!(&c.detail, Detail::Row { site, .. } if site == "handle Shutdown"))
        .expect("the handler row change is reported");
    assert_eq!(handler.kind, NodeKind::Actor);
    assert_eq!(handler.name, "Planner");
    assert_eq!(handler.severity, Severity::Widened);
    assert!(
        matches!(&handler.detail, Detail::Row { added, removed, opened: None, .. }
            if added == &["Tool<Probe>"] && removed.is_empty()),
        "{handler:?}"
    );
    assert_eq!(
        handler.to_string(),
        "Planner actor Planner: handle Shutdown gains Tool<Probe>"
    );

    assert!(
        report.changes.iter().any(|c| c.kind == NodeKind::Tool
            && c.name == "Probe"
            && c.severity == Severity::Widened
            && c.detail == Detail::Added),
        "the new tool declaration is reported: {:?}",
        report.changes
    );
    assert!(
        report.changes.iter().any(
            |c| matches!(&c.detail, Detail::Row { site, .. } if site == "summary")
                && c.severity == Severity::Widened
        ),
        "the actor summary widens too: {:?}",
        report.changes
    );
}

#[test]
fn losing_an_effect_narrows_without_widening() {
    let base = graph(PLANNER, "Planner");
    let narrowed = edited(
        "handle PlanRepo(p), st ! {Tool<ReadRepo>} = match read(p) { n -> Continue(St(n)) },",
        "handle PlanRepo(p), st ! {} = Continue(st),",
    )
    .replace("} ! {Tool<ReadRepo>}", "} ! {}");
    let report = diff(&base, &graph(&narrowed, "Planner"));
    assert!(!report.widens, "{:?}", report.changes);
    assert!(
        report
            .changes
            .iter()
            .all(|c| c.severity == Severity::Narrowed),
        "{:?}",
        report.changes
    );
    let sites: Vec<&str> = report
        .changes
        .iter()
        .filter(|c| c.kind == NodeKind::Actor)
        .filter_map(|c| match &c.detail {
            Detail::Row { site, .. } => Some(site.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(sites, ["handle PlanRepo", "summary"]);
    assert!(
        report
            .changes
            .iter()
            .any(|c| c.kind == NodeKind::Supervisor),
        "the derived supervisor row narrows too: {:?}",
        report.changes
    );
}

#[test]
fn a_protocol_change_widens() {
    let base = graph(PLANNER, "Planner");
    let reshaped = edited(
        "message: PlannerMsg = | PlanRepo(Path) | Shutdown,",
        "message: PlannerMsg = | PlanRepo(Path) | Shutdown | Ping,",
    )
    .replace(
        "handle Shutdown, st ! {} = Stop,",
        "handle Shutdown, st ! {} = Stop,\n  handle Ping, st ! {} = Continue(st),",
    );
    let report = diff(&base, &graph(&reshaped, "Planner"));
    assert!(report.widens);
    let [change] = report.changes.as_slice() else {
        panic!(
            "expected exactly the protocol change, got {:?}",
            report.changes
        );
    };
    assert_eq!(change.severity, Severity::Widened);
    assert_eq!(
        change.detail,
        Detail::Constructors {
            added: vec!["Ping".into()],
            removed: vec![],
            changed: vec![],
        }
    );
    assert_eq!(
        change.to_string(),
        "Planner actor Planner: protocol gains Ping"
    );
}

#[test]
fn supervision_changes_are_neutral() {
    let base = graph(PLANNER, "Planner");
    let restrategised = edited("strategy: one_for_one,", "strategy: one_for_all,")
        .replace("restart: permanent", "restart: transient");
    let report = diff(&base, &graph(&restrategised, "Planner"));
    assert!(!report.widens, "{:?}", report.changes);
    let details: Vec<&Detail> = report.changes.iter().map(|c| &c.detail).collect();
    assert_eq!(
        details,
        [
            &Detail::Supervision {
                field: "strategy".into(),
                before: "one_for_one".into(),
                after: "one_for_all".into(),
            },
            &Detail::Children {
                added: vec![],
                removed: vec![],
                changed: vec!["planner".into()],
            },
        ]
    );
    assert!(
        report
            .changes
            .iter()
            .all(|c| c.severity == Severity::Changed)
    );
}

#[test]
fn a_type_change_is_neutral() {
    let base = graph(PLANNER, "Planner");
    let retyped = edited(
        "fn default_config() -> St = St(0)",
        "fn default_config() -> St = St(1)",
    )
    .replace(
        "fn read(p: Path) -> Int ! {Tool<ReadRepo>} = read_repo({ path: p })",
        "fn read(p: Path, again: Bool) -> Int ! {Tool<ReadRepo>} = read_repo({ path: p })",
    )
    .replace("match read(p) {", "match read(p, True) {");
    let report = diff(&base, &graph(&retyped, "Planner"));
    assert!(!report.widens, "{:?}", report.changes);
    let [change] = report.changes.as_slice() else {
        panic!(
            "expected exactly the signature change, got {:?}",
            report.changes
        );
    };
    assert_eq!(change.kind, NodeKind::Function);
    assert_eq!(change.name, "read");
    assert_eq!(
        change.to_string(),
        "Planner function read: signature (p: Path) \u{2192} Int \u{2192} (p: Path, again: Bool) \u{2192} Int"
    );
}

#[test]
fn modules_appearing_and_vanishing() {
    let base = graph(PLANNER, "Planner");
    let other = graph(PLANNER, "Other");
    let removed = diff(&base, &other);
    assert!(
        !removed.widens || removed.widens,
        "either verdict is a valid bool"
    );
    let module_level: Vec<(&str, Severity, &Detail)> = removed
        .changes
        .iter()
        .filter(|c| c.kind == NodeKind::Module)
        .map(|c| (c.name.as_str(), c.severity, &c.detail))
        .collect();
    assert_eq!(
        module_level,
        [
            ("Other", Severity::Changed, &Detail::Added),
            ("Planner", Severity::Narrowed, &Detail::Removed),
        ]
    );
    assert!(
        removed.widens,
        "the new module's actor and tool widen: {:?}",
        removed.changes
    );
    assert!(
        removed
            .changes
            .iter()
            .filter(|c| c.module == "Planner" && c.kind != NodeKind::Module)
            .all(|c| c.severity == Severity::Narrowed && c.detail == Detail::Removed),
        "everything in the vanished module is a removal: {:?}",
        removed.changes
    );
}

#[test]
fn pure_additions_are_neutral() {
    let base = graph(PLANNER, "Planner");
    let extended = format!("{PLANNER}\nfn twice(n: Int) -> Int = n + n");
    let report = diff(&base, &graph(&extended, "Planner"));
    assert!(!report.widens, "{:?}", report.changes);
    let [change] = report.changes.as_slice() else {
        panic!(
            "expected exactly the added function, got {:?}",
            report.changes
        );
    };
    assert_eq!(change.kind, NodeKind::Function);
    assert_eq!(change.severity, Severity::Changed);
    assert_eq!(change.detail, Detail::Added);
}

#[test]
fn report_round_trips_through_json() {
    let base = graph(PLANNER, "Planner");
    let extended = format!("{PLANNER}\nfn twice(n: Int) -> Int = n + n");
    let report = diff(&base, &graph(&extended, "Planner"));
    let json = serde_json::to_string(&report).expect("serializes");
    assert!(json.contains("\"severity\":\"changed\""), "{json}");
    assert!(json.contains("\"change\":\"added\""), "{json}");
    assert!(json.contains("\"kind\":\"function\""), "{json}");
    let back: GraphDiff = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(back, report);
}

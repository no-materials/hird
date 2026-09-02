// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Effect-graph projection: actors, supervisors, and tools out of a lowered
//! module, with the JSON shape pinned by a snapshot.

use hird_ast::{AstNode, SourceFile};
use hird_ir::{EFFECT_GRAPH_SCHEMA_VERSION, IrModule, TypeStructure, effect_graph, lower_module};

/// A planner-shaped module: a tool, an actor using it, and a supervisor.
const PLANNER: &str = "type Path = Path(String)\n\
     type RepoState = RepoState(String)\n\
     type St = St(Int)\n\
     type Status = Status(Int)\n\
     tool ReadRepo : { path: Path } -> RepoState\n\
     fn read(p: Path, st: St) -> St ! {Tool<ReadRepo>} =\n\
       match read_repo({ path: p }) { _ -> st }\n\
     fn default_config() -> St = St(0)\n\
     actor Planner {\n\
       state: St,\n\
       message: PlannerMsg = | PlanRepo(Path) | GetStatus(ReplyTo<Status>) | Shutdown,\n\
       init: fn(c: St) ! {} = c,\n\
       handle PlanRepo(p), st ! {Tool<ReadRepo>} = Continue(read(p, st)),\n\
       handle GetStatus(r), St(n) ! {Send<Status>} = let ack = reply(r, Status(n)) in Continue(St(n)),\n\
       handle Shutdown, st ! {} = Continue(st),\n\
     } ! {Tool<ReadRepo>, Send<Status>}\n\
     supervisor PlannerSup {\n\
       strategy: one_for_one,\n\
       intensity: 5,\n\
       period: 60,\n\
       children: [\n\
         { id: planner, actor: Planner, start_args: default_config(), restart: permanent },\n\
       ]\n\
     }";

/// A generic tool, checking its parameters render by their declared names.
const GENERIC: &str = "tool Pick<t> : { first: t, second: t } -> t";

/// Parses, checks, and lowers `source`, panicking on any parse or type error.
fn lower(source: &str, name: &str) -> IrModule {
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
    lower_module(&file, &checked, name)
}

#[test]
fn planner_graph_projects_actor_supervisor_and_tool() {
    let graph = effect_graph(&lower(PLANNER, "Planner"));

    assert_eq!(
        graph.schema_version, EFFECT_GRAPH_SCHEMA_VERSION,
        "graph carries the schema version"
    );
    assert_eq!(graph.module, "Planner", "graph names its module");

    let [actor] = graph.actors.as_slice() else {
        panic!("expected one actor, got {:?}", graph.actors);
    };
    assert_eq!(actor.name, "Planner", "actor name");
    assert!(actor.line > 0, "actor carries its source line");
    assert_eq!(actor.state.display, "St", "state type display");
    assert_eq!(actor.message.name, "PlannerMsg", "message type name");
    let ctor_names: Vec<&str> = actor
        .message
        .constructors
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        ctor_names,
        ["PlanRepo", "GetStatus", "Shutdown"],
        "message constructors in source order"
    );
    assert_eq!(
        actor.message.constructors[1].fields[0].display, "ReplyTo<Status>",
        "constructor field types render canonically"
    );
    assert_eq!(actor.init.params.len(), 1, "init parameter count");
    assert_eq!(actor.init.params[0].ty.display, "St", "init parameter type");
    assert!(actor.init.effects.effects.is_empty(), "init row is pure");
    let handled: Vec<&str> = actor.handlers.iter().map(|h| h.message.as_str()).collect();
    assert_eq!(
        handled,
        ["PlanRepo", "GetStatus", "Shutdown"],
        "handlers keyed by constructor"
    );
    assert_eq!(
        actor.handlers[0].effects.display, "{Tool<ReadRepo>}",
        "handler row display"
    );
    assert_eq!(
        actor.effects.display, "{Send<Status>, Tool<ReadRepo>}",
        "actor summary row display (head order)"
    );
    let tool_effect = actor
        .effects
        .effects
        .iter()
        .find(|e| e.head == "Tool")
        .expect("summary contains a Tool effect");
    assert_eq!(
        tool_effect.args[0].display, "ReadRepo",
        "effect arguments render canonically"
    );
    assert!(!actor.effects.open, "summary row is closed");

    let [sup] = graph.supervisors.as_slice() else {
        panic!("expected one supervisor, got {:?}", graph.supervisors);
    };
    assert_eq!(sup.name, "PlannerSup", "supervisor name");
    assert!(sup.line > 0, "supervisor carries its source line");
    assert_eq!(
        (sup.strategy.as_str(), sup.intensity, sup.period),
        ("one_for_one", 5, 60),
        "strategy and restart budget"
    );
    let [child] = sup.children.as_slice() else {
        panic!("expected one child, got {:?}", sup.children);
    };
    assert_eq!(
        (
            child.id.as_str(),
            child.actor.as_str(),
            child.restart.as_str()
        ),
        ("planner", "Planner", "permanent"),
        "child spec"
    );
    assert_eq!(
        sup.effects.display, actor.effects.display,
        "supervisor row derives from its children"
    );

    let [tool] = graph.tools.as_slice() else {
        panic!("expected one tool, got {:?}", graph.tools);
    };
    assert_eq!(tool.name, "ReadRepo", "tool name");
    assert!(tool.line > 0, "tool carries its source line");
    assert!(tool.params.is_empty(), "monomorphic tool has no params");
    assert_eq!(tool.output.display, "RepoState", "tool output display");
    let TypeStructure::Record { fields } = &tool.input.structure else {
        panic!("expected a record input, got {:?}", tool.input.structure);
    };
    assert_eq!(
        fields
            .get("path")
            .map(|f| format!("{f:?}").contains("Path")),
        Some(true),
        "input record keeps its field structure"
    );
}

#[test]
fn generic_tool_renders_parameters_by_declared_name() {
    let graph = effect_graph(&lower(GENERIC, "Tools"));
    let [tool] = graph.tools.as_slice() else {
        panic!("expected one tool, got {:?}", graph.tools);
    };
    assert_eq!(tool.params, ["t"], "generic tool records its parameters");
    assert_eq!(
        tool.output.display, "t",
        "output renders the parameter name"
    );
    let TypeStructure::Record { fields } = &tool.input.structure else {
        panic!("expected a record input, got {:?}", tool.input.structure);
    };
    assert!(
        fields.values().all(
            |f| matches!(f, TypeStructure::Con { name, args } if name == "t" && args.is_empty())
        ),
        "both fields render the parameter name"
    );
}

#[test]
fn planner_graph_json_shape() {
    let graph = effect_graph(&lower(PLANNER, "Planner"));
    insta::assert_snapshot!(graph.to_json_pretty().expect("graph serializes"));
}

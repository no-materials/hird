// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! End-to-end MCP tests: JSON-RPC messages into the server, tool results
//! out, exercised against the v0.1 planner demo.

use std::path::PathBuf;

use hird_mcp::Server;
use serde_json::{Value, json};

/// The v0.1 supervised-planner demo source, the fixture every tool answers
/// about.
fn demo_path() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../demo/agent_planner.hird");
    path.canonicalize()
        .expect("the demo program exists")
        .to_str()
        .expect("a UTF-8 path")
        .to_owned()
}

/// Sends one request and returns its `result`, panicking on a JSON-RPC
/// error.
fn request(server: &mut Server, id: i64, method: &str, params: Value) -> Value {
    let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let response = server
        .handle_message(&message.to_string())
        .expect("a request gets a response");
    let response: Value = serde_json::from_str(&response).expect("a JSON response");
    assert!(
        response.get("error").is_none(),
        "unexpected JSON-RPC error: {response}"
    );
    response["result"].clone()
}

/// Calls one tool and returns its `structuredContent`, asserting success.
fn call_tool(server: &mut Server, name: &str, arguments: Value) -> Value {
    let result = request(
        server,
        99,
        "tools/call",
        json!({ "name": name, "arguments": arguments }),
    );
    assert!(
        result.get("isError").is_none_or(|e| e == false),
        "tool `{name}` failed: {result}"
    );
    result["structuredContent"].clone()
}

/// Calls one tool expecting an `isError` result, returning its structured
/// error payload.
fn call_tool_err(server: &mut Server, name: &str, arguments: Value) -> Value {
    let result = request(
        server,
        99,
        "tools/call",
        json!({ "name": name, "arguments": arguments }),
    );
    assert_eq!(result["isError"], true, "expected a tool error: {result}");
    result["structuredContent"]["error"].clone()
}

/// The 1-based line and column of `target` within the first occurrence of
/// `context` in `source`.
fn position(source: &str, context: &str, target: &str) -> (usize, usize) {
    let context_start = source.find(context).expect("context not in source");
    let offset = context_start + context.find(target).expect("target not in context");
    let line = source[..offset].matches('\n').count() + 1;
    let line_start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    let column = source[line_start..offset].chars().count() + 1;
    (line, column)
}

#[test]
fn initialize_negotiates_and_identifies() {
    let mut server = Server::new();
    let result = request(
        &mut server,
        1,
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {} }),
    );
    assert_eq!(result["protocolVersion"], "2025-06-18");
    assert_eq!(result["serverInfo"]["name"], "hird-mcp");
    assert!(result["capabilities"]["tools"].is_object());

    // A notification gets no response.
    let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert_eq!(server.handle_message(&notification.to_string()), None);
}

#[test]
fn tools_list_serves_all_eight_tools() {
    let mut server = Server::new();
    let result = request(&mut server, 1, "tools/list", json!({}));
    let tools = result["tools"].as_array().expect("a tool array");
    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(
        names,
        [
            "infer_type",
            "lookup_definition",
            "explain_effect_row",
            "render_ir_fragment",
            "explain_actor_protocol",
            "emit_actor_effect_graph",
            "get_context_for_symbol",
            "get_context_budget",
        ]
    );
    for tool in tools {
        assert!(tool["inputSchema"]["properties"]["file"].is_object());
    }
}

#[test]
fn infer_type_reports_type_and_effect_row() {
    let mut server = Server::new();
    let file = demo_path();
    let source = std::fs::read_to_string(&file).expect("the demo reads");

    // The tool-generated function reference: a function type with a row.
    let (line, column) = position(&source, "read_repo({ path: path })", "read_repo");
    let result = call_tool(
        &mut server,
        "infer_type",
        json!({ "file": file, "line": line, "column": column }),
    );
    assert_eq!(result["token"], "read_repo");
    assert_eq!(
        result["type"],
        "{ path: Path } \u{2192} RepoState ! {Tool<ReadRepo>}"
    );
    assert_eq!(result["effect_row"], "{Tool<ReadRepo>}");

    // A plain parameter reference: a data type, empty row.
    let (line, column) = position(&source, "read_repo({ path: path })", "path })");
    let result = call_tool(
        &mut server,
        "infer_type",
        json!({ "file": file, "line": line, "column": column }),
    );
    assert_eq!(result["type"], "Path");
    assert_eq!(result["effect_row"], "{}");
}

#[test]
fn lookup_definition_reports_location_type_doc_and_kind() {
    let mut server = Server::new();
    let file = demo_path();

    let result = call_tool(
        &mut server,
        "lookup_definition",
        json!({ "file": file, "name": "analyze" }),
    );
    assert_eq!(result["kind"], "function");
    assert_eq!(result["type"], "RepoState \u{2192} Backlog");
    assert_eq!(
        result["doc"],
        "Repository state to the tickets worth filing."
    );
    assert!(result["line"].as_u64().expect("a line") > 1);

    let result = call_tool(
        &mut server,
        "lookup_definition",
        json!({ "file": file, "name": "CreateTicket" }),
    );
    assert_eq!(result["kind"], "tool");

    let result = call_tool(
        &mut server,
        "lookup_definition",
        json!({ "file": file, "name": "Planner" }),
    );
    assert_eq!(result["kind"], "actor");
}

#[test]
fn explain_effect_row_explains_each_effect() {
    let mut server = Server::new();
    let result = call_tool(
        &mut server,
        "explain_effect_row",
        json!({ "file": demo_path(), "fn_name": "file_tickets" }),
    );
    assert_eq!(result["effect_row"], "{Tool<CreateTicket>, Tool<Log>}");
    assert_eq!(result["pure"], false);
    assert_eq!(result["open"], false);
    let effects = result["effects"].as_array().expect("effects");
    assert_eq!(effects.len(), 2);
    assert_eq!(effects[0]["effect"], "Tool<CreateTicket>");
    assert!(
        effects[0]["explanation"]
            .as_str()
            .expect("an explanation")
            .contains("external tool `CreateTicket`")
    );

    // A pure function has an empty, closed row.
    let result = call_tool(
        &mut server,
        "explain_effect_row",
        json!({ "file": demo_path(), "fn_name": "analyze" }),
    );
    assert_eq!(result["pure"], true);
    assert_eq!(result["effects"], json!([]));
}

#[test]
fn render_ir_fragment_serializes_one_declaration() {
    let mut server = Server::new();
    let result = call_tool(
        &mut server,
        "render_ir_fragment",
        json!({ "file": demo_path(), "name": "file_tickets" }),
    );
    assert_eq!(result["module"], "AgentPlanner");
    assert_eq!(result["ir"]["kind"], "Fn");
    assert_eq!(result["ir"]["name"], "file_tickets");
    assert_eq!(result["ir"]["return_type"], "Int");

    // A tool resolves through its generated function name too.
    let result = call_tool(
        &mut server,
        "render_ir_fragment",
        json!({ "file": demo_path(), "name": "create_ticket" }),
    );
    assert_eq!(result["ir"]["kind"], "Tool");
    assert_eq!(result["ir"]["name"], "CreateTicket");
}

#[test]
fn explain_actor_protocol_covers_the_planner() {
    let mut server = Server::new();
    let result = call_tool(
        &mut server,
        "explain_actor_protocol",
        json!({ "file": demo_path(), "actor_name": "Planner" }),
    );
    let actor = &result["actor"];
    assert_eq!(actor["name"], "Planner");
    assert_eq!(actor["state"]["display"], "PlannerState");
    assert_eq!(actor["message"]["name"], "PlannerMsg");
    let constructors: Vec<&str> = actor["message"]["constructors"]
        .as_array()
        .expect("constructors")
        .iter()
        .map(|c| c["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(constructors, ["PlanRepo", "GetStatus", "Shutdown"]);
    let handlers = actor["handlers"].as_array().expect("handlers");
    assert_eq!(handlers.len(), 3);
    assert_eq!(handlers[0]["message"], "PlanRepo");
    assert_eq!(
        handlers[0]["effects"]["display"],
        "{Tool<CreateTicket>, Tool<Log>, Tool<ReadRepo>}"
    );
    assert_eq!(
        actor["effects"]["display"],
        "{Send<PlannerStatus>, Tool<CreateTicket>, Tool<Log>, Tool<ReadRepo>}"
    );
}

#[test]
fn actor_effect_graph_includes_supervisor_and_tools() {
    let mut server = Server::new();
    let result = call_tool(
        &mut server,
        "emit_actor_effect_graph",
        json!({ "file": demo_path(), "actor_name": "Planner" }),
    );
    assert_eq!(result["root"], "Planner");
    assert_eq!(result["module"], "AgentPlanner");

    let actors: Vec<&str> = result["actors"]
        .as_array()
        .expect("actors")
        .iter()
        .map(|a| a["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(actors, ["Planner"]);

    let supervisors = result["supervisors"].as_array().expect("supervisors");
    assert_eq!(supervisors.len(), 1);
    assert_eq!(supervisors[0]["name"], "PlannerSup");
    assert_eq!(supervisors[0]["strategy"], "one_for_one");
    assert_eq!(supervisors[0]["children"][0]["actor"], "Planner");

    let tools: Vec<&str> = result["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|t| t["name"].as_str().expect("a name"))
        .collect();
    // Source order, like the module-wide effect graph.
    assert_eq!(tools, ["ReadRepo", "CreateTicket", "Log"]);
}

#[test]
fn context_for_symbol_fits_the_budget() {
    let mut server = Server::new();

    // A generous budget includes everything: signature, effects, doc,
    // callers, callees.
    let result = call_tool(
        &mut server,
        "get_context_for_symbol",
        json!({ "file": demo_path(), "name": "file_tickets", "budget": 400 }),
    );
    assert_eq!(result["symbol"], "file_tickets");
    assert_eq!(result["kind"], "function");
    let summary = result["summary"].as_str().expect("a summary");
    assert!(summary.contains("fn file_tickets"), "summary: {summary}");
    assert!(
        summary.contains("effects: {Tool<CreateTicket>, Tool<Log>}"),
        "summary: {summary}"
    );
    assert!(summary.contains("callers: Planner"), "summary: {summary}");
    assert!(
        summary.contains("callees: create_ticket, log"),
        "summary: {summary}"
    );
    assert_eq!(result["omitted"], json!([]));

    // A tight budget keeps the signature and reports what it dropped.
    let result = call_tool(
        &mut server,
        "get_context_for_symbol",
        json!({ "file": demo_path(), "name": "file_tickets", "budget": 20 }),
    );
    let approx = result["approx_tokens"].as_u64().expect("a token count");
    assert!(approx <= 20, "over budget: {result}");
    assert!(
        result["summary"]
            .as_str()
            .expect("a summary")
            .contains("file_tickets")
    );
    assert!(
        !result["omitted"].as_array().expect("omitted").is_empty(),
        "a tight budget must drop sections: {result}"
    );
}

#[test]
fn context_budget_counts_every_category() {
    let mut server = Server::new();
    let result = call_tool(
        &mut server,
        "get_context_budget",
        json!({ "file": demo_path() }),
    );
    assert_eq!(result["module"], "AgentPlanner");
    let tokens = &result["approx_tokens"];
    let mut sum = 0;
    for category in [
        "types",
        "effects",
        "actors",
        "supervisors",
        "tools",
        "functions",
    ] {
        let count = tokens[category].as_u64().expect("a count");
        assert!(count > 0, "empty category `{category}`: {result}");
        sum += count;
    }
    assert_eq!(tokens["total"].as_u64().expect("a total"), sum);
}

#[test]
fn errors_are_structured_not_crashes() {
    let mut server = Server::new();
    let file = demo_path();

    // A missing file.
    let error = call_tool_err(
        &mut server,
        "get_context_budget",
        json!({ "file": "does/not/exist.hird" }),
    );
    assert_eq!(error["code"], "file_not_found");

    // An undefined name, with the available names to steer by.
    let error = call_tool_err(
        &mut server,
        "lookup_definition",
        json!({ "file": file, "name": "nonsense" }),
    );
    assert_eq!(error["code"], "not_found");
    assert!(
        error["data"]["available"]
            .as_array()
            .expect("available names")
            .iter()
            .any(|n| n == "Planner")
    );

    // A non-actor name against an actor tool.
    let error = call_tool_err(
        &mut server,
        "explain_actor_protocol",
        json!({ "file": file, "actor_name": "analyze" }),
    );
    assert_eq!(error["code"], "not_found");
    assert_eq!(error["data"]["available_actors"], json!(["Planner"]));

    // A parse error comes back with diagnostics.
    let broken = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("broken.hird");
    std::fs::write(&broken, "fn broken( = 1\n").expect("the fixture writes");
    let error = call_tool_err(
        &mut server,
        "get_context_budget",
        json!({ "file": broken.to_str().expect("a UTF-8 path") }),
    );
    assert_eq!(error["code"], "parse_error");
    assert!(
        !error["data"]["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .is_empty()
    );

    // A type error comes back with coded diagnostics.
    let ill_typed = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ill_typed.hird");
    std::fs::write(&ill_typed, "fn broken() \u{2192} Int = \"nope\"\n")
        .expect("the fixture writes");
    let error = call_tool_err(
        &mut server,
        "get_context_budget",
        json!({ "file": ill_typed.to_str().expect("a UTF-8 path") }),
    );
    assert_eq!(error["code"], "check_error");
    assert_eq!(error["data"]["diagnostics"][0]["code"], "C0001");

    // Missing arguments and unknown tools are protocol-level failures.
    let message = json!({
        "jsonrpc": "2.0", "id": 5, "method": "tools/call",
        "params": { "name": "no_such_tool", "arguments": {} },
    });
    let response = server
        .handle_message(&message.to_string())
        .expect("a response");
    let response: Value = serde_json::from_str(&response).expect("JSON");
    assert_eq!(response["error"]["code"], -32602);
}

#[test]
fn edits_invalidate_the_cache() {
    let mut server = Server::new();
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("evolving.hird");
    let file = path.to_str().expect("a UTF-8 path").to_owned();

    std::fs::write(&path, "fn answer() \u{2192} Int = 42\n").expect("the fixture writes");
    let result = call_tool(
        &mut server,
        "lookup_definition",
        json!({ "file": file, "name": "answer" }),
    );
    assert_eq!(result["type"], "() \u{2192} Int");

    std::fs::write(&path, "fn answer() \u{2192} String = \"42\"\n").expect("the fixture writes");
    let result = call_tool(
        &mut server,
        "lookup_definition",
        json!({ "file": file, "name": "answer" }),
    );
    assert_eq!(result["type"], "() \u{2192} String");
}

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
    // The state is a type alias, which displays as its expansion.
    assert_eq!(actor["state"]["display"], "{ repos: Int, tickets: Int }");
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

/// The path of one file of the two-module fixture (`app.hird` imports from
/// `util.hird`).
fn two_modules_path(file: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/two_modules")
        .join(file)
        .to_str()
        .expect("a UTF-8 path")
        .to_owned()
}

#[test]
fn imported_symbols_resolve_to_their_defining_file() {
    let mut server = Server::new();
    let app = two_modules_path("app.hird");
    let util = two_modules_path("util.hird");

    // A selectively imported function: defined, typed, and documented in the
    // sibling file.
    let result = call_tool(
        &mut server,
        "lookup_definition",
        json!({ "file": app, "name": "double" }),
    );
    assert_eq!(result["file"], util);
    assert_eq!(result["kind"], "function");
    assert_eq!(result["type"], "Int \u{2192} Int");
    assert_eq!(result["doc"], "Doubles a number.");
    assert_eq!(result["line"], 6);

    // A qualified member through a whole-module import.
    let result = call_tool(
        &mut server,
        "lookup_definition",
        json!({ "file": app, "name": "Util.show" }),
    );
    assert_eq!(result["file"], util);
    assert_eq!(result["name"], "show");
    assert_eq!(result["type"], "Path \u{2192} String");

    // An imported type.
    let result = call_tool(
        &mut server,
        "lookup_definition",
        json!({ "file": app, "name": "Path" }),
    );
    assert_eq!(result["file"], util);
    assert_eq!(result["kind"], "type");

    // The IR of an imported definition comes from the defining module.
    let result = call_tool(
        &mut server,
        "render_ir_fragment",
        json!({ "file": app, "name": "Util.show" }),
    );
    assert_eq!(result["file"], util);
    assert_eq!(result["module"], "Util");
    assert_eq!(result["ir"]["kind"], "Fn");
    assert_eq!(result["ir"]["name"], "show");

    // Effect rows resolve the same way.
    let result = call_tool(
        &mut server,
        "explain_effect_row",
        json!({ "file": app, "fn_name": "double" }),
    );
    assert_eq!(result["file"], util);
    assert_eq!(result["pure"], true);
}

#[test]
fn infer_type_sees_through_imports() {
    let mut server = Server::new();
    let app = two_modules_path("app.hird");
    let source = std::fs::read_to_string(&app).expect("the fixture reads");

    // A selectively imported function reference.
    let (line, column) = position(&source, "= double(n)", "double");
    let result = call_tool(
        &mut server,
        "infer_type",
        json!({ "file": app, "line": line, "column": column }),
    );
    assert_eq!(result["file"], app);
    assert_eq!(result["token"], "double");
    assert_eq!(result["type"], "Int \u{2192} Int");

    // The member of a qualified access.
    let (line, column) = position(&source, "Util.show(p)", "show");
    let result = call_tool(
        &mut server,
        "infer_type",
        json!({ "file": app, "line": line, "column": column }),
    );
    assert_eq!(result["type"], "Path \u{2192} String");

    // An imported type annotation resolves rather than erroring.
    let (line, column) = position(&source, "(p: Path)", "Path");
    let result = call_tool(
        &mut server,
        "infer_type",
        json!({ "file": app, "line": line, "column": column }),
    );
    assert_eq!(result["token"], "Path");
}

#[test]
fn context_for_imported_symbol_spans_the_program() {
    let mut server = Server::new();
    let app = two_modules_path("app.hird");
    let util = two_modules_path("util.hird");

    // Callers of an imported function are found in the importing module.
    let result = call_tool(
        &mut server,
        "get_context_for_symbol",
        json!({ "file": app, "name": "double" }),
    );
    assert_eq!(result["file"], util);
    let summary = result["summary"].as_str().expect("a summary");
    assert!(
        summary.contains("fn double : Int \u{2192} Int"),
        "summary: {summary}"
    );
    assert!(
        summary.contains("doc: Doubles a number."),
        "summary: {summary}"
    );
    assert!(summary.contains("callers: run"), "summary: {summary}");

    // Callees resolve to their defining module, whether the body writes
    // them qualified or brings them in with a selective import.
    let result = call_tool(
        &mut server,
        "get_context_for_symbol",
        json!({ "file": app, "name": "describe" }),
    );
    assert_eq!(result["file"], app);
    let summary = result["summary"].as_str().expect("a summary");
    assert!(summary.contains("callees: Util.show"), "summary: {summary}");
    let result = call_tool(
        &mut server,
        "get_context_for_symbol",
        json!({ "file": app, "name": "run" }),
    );
    let summary = result["summary"].as_str().expect("a summary");
    assert!(
        summary.contains("callees: Util.double"),
        "summary: {summary}"
    );
    assert!(summary.contains("callers: local"), "summary: {summary}");
}

#[test]
fn imports_are_scoped_to_the_querying_file() {
    let mut server = Server::new();
    let app = two_modules_path("app.hird");
    let util = two_modules_path("util.hird");

    // `util.hird` does not import `App`, so `run` is not in its scope.
    let error = call_tool_err(
        &mut server,
        "lookup_definition",
        json!({ "file": util, "name": "run" }),
    );
    assert_eq!(error["code"], "not_found");

    // The importing file's scope lists its imports alongside its own names.
    let error = call_tool_err(
        &mut server,
        "lookup_definition",
        json!({ "file": app, "name": "nonsense" }),
    );
    let available = error["data"]["available"].as_array().expect("names");
    for name in ["run", "double", "Path"] {
        assert!(available.iter().any(|n| n == name), "missing `{name}`");
    }
    assert!(
        !available.iter().any(|n| n == "show"),
        "qualified-only names stay out"
    );
}

#[test]
fn sibling_edits_invalidate_the_program() {
    let mut server = Server::new();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sibling_edits");
    std::fs::create_dir_all(&dir).expect("the fixture directory creates");
    let lib = dir.join("lib.hird");
    let main = dir.join("main.hird");
    std::fs::write(&lib, "pub fn answer() \u{2192} Int = 42\n").expect("the fixture writes");
    std::fs::write(
        &main,
        "use Lib.{answer}\nfn run() \u{2192} Int = answer()\n",
    )
    .expect("the fixture writes");
    let main = main.to_str().expect("a UTF-8 path").to_owned();

    let result = call_tool(
        &mut server,
        "lookup_definition",
        json!({ "file": main, "name": "answer" }),
    );
    assert_eq!(result["line"], 1);
    assert_eq!(result["doc"], Value::Null);

    // Changing only the sibling recompiles the program.
    std::fs::write(&lib, "// The answer.\npub fn answer() \u{2192} Int = 42\n")
        .expect("the fixture writes");
    let result = call_tool(
        &mut server,
        "lookup_definition",
        json!({ "file": main, "name": "answer" }),
    );
    assert_eq!(result["line"], 2);
    assert_eq!(result["doc"], "The answer.");

    // A sibling edit that breaks the importer is the importer's type error.
    std::fs::write(&lib, "pub fn answer() \u{2192} String = \"42\"\n").expect("the fixture writes");
    let error = call_tool_err(&mut server, "get_context_budget", json!({ "file": main }));
    assert_eq!(error["code"], "check_error");

    // A sibling that no longer parses drops out of the program; the
    // importer reports the unresolved import and names the broken file.
    std::fs::write(&lib, "pub fn answer( = 1\n").expect("the fixture writes");
    let error = call_tool_err(&mut server, "get_context_budget", json!({ "file": main }));
    assert_eq!(error["code"], "check_error");
    assert_eq!(error["data"]["diagnostics"][0]["code"], "C0023");
    assert_eq!(
        error["data"]["siblings_with_parse_errors"][0]["file"],
        lib.to_str().expect("a UTF-8 path")
    );
}

/// Whether `value` satisfies the JSON-schema subset the descriptors use:
/// `type` (a name or a list of names), `required`, `properties`, `items`.
fn conforms(value: &Value, schema: &Value) -> bool {
    let type_ok = match &schema["type"] {
        Value::String(t) => has_type(value, t),
        Value::Array(ts) => ts.iter().any(|t| has_type(value, t.as_str().unwrap_or(""))),
        _ => true,
    };
    let required_ok = schema["required"].as_array().is_none_or(|keys| {
        keys.iter()
            .all(|k| value.get(k.as_str().unwrap()).is_some())
    });
    let properties_ok = schema["properties"].as_object().is_none_or(|props| {
        props
            .iter()
            .all(|(k, s)| value.get(k).is_none_or(|v| conforms(v, s)))
    });
    let items_ok = match (&schema["items"], value.as_array()) {
        (Value::Object(_), Some(items)) => items.iter().all(|v| conforms(v, &schema["items"])),
        _ => true,
    };
    type_ok && required_ok && properties_ok && items_ok
}

/// Whether `value` has JSON-schema type `name`.
fn has_type(value: &Value, name: &str) -> bool {
    match name {
        "string" => value.is_string(),
        "integer" => value.is_u64() || value.is_i64(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        _ => false,
    }
}

#[test]
fn descriptors_declare_read_only_annotations_and_matching_output_schemas() {
    let mut server = Server::new();
    let file = demo_path();
    let source = std::fs::read_to_string(&file).expect("the demo reads");
    let (line, column) = position(&source, "read_repo({ path: path })", "read_repo");
    let arguments = |tool: &str| match tool {
        "infer_type" => json!({ "file": file, "line": line, "column": column }),
        "lookup_definition" | "render_ir_fragment" => {
            json!({ "file": file, "name": "file_tickets" })
        }
        "explain_effect_row" => json!({ "file": file, "fn_name": "file_tickets" }),
        "explain_actor_protocol" | "emit_actor_effect_graph" => {
            json!({ "file": file, "actor_name": "Planner" })
        }
        "get_context_for_symbol" => json!({ "file": file, "name": "file_tickets" }),
        "get_context_budget" => json!({ "file": file }),
        _ => panic!("no demo arguments for `{tool}`"),
    };

    let listed = request(&mut server, 1, "tools/list", json!({}));
    for tool in listed["tools"].as_array().expect("a tool array") {
        let name = tool["name"].as_str().expect("a name");
        assert!(tool["title"].is_string(), "`{name}` has no title");
        assert_eq!(tool["annotations"]["readOnlyHint"], true, "`{name}`");
        assert_eq!(tool["annotations"]["destructiveHint"], false, "`{name}`");
        assert_eq!(tool["annotations"]["idempotentHint"], true, "`{name}`");
        assert_eq!(tool["annotations"]["openWorldHint"], false, "`{name}`");

        // The description discloses behaviour, output, and failure modes.
        let description = tool["description"].as_str().expect("a description");
        for expected in ["Read-only", "Returns", "`error.code`"] {
            assert!(
                description.contains(expected),
                "`{name}` description lacks `{expected}`: {description}"
            );
        }

        // Every input property is required and described.
        let input = &tool["inputSchema"];
        let properties = input["properties"].as_object().expect("input properties");
        for (key, schema) in properties {
            assert!(
                schema["description"].is_string(),
                "`{name}.{key}` undescribed"
            );
        }
        let required: Vec<&str> = input["required"]
            .as_array()
            .expect("required inputs")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for key in properties.keys() {
            assert!(
                required.contains(&key.as_str()),
                "`{name}.{key}` not required"
            );
        }

        let result = call_tool(&mut server, name, arguments(name));
        assert!(
            conforms(&result, &tool["outputSchema"]),
            "`{name}` result does not match its outputSchema:\n{result:#}\n{:#}",
            tool["outputSchema"]
        );
    }
}

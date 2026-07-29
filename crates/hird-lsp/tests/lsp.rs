// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! End-to-end LSP tests: JSON-RPC requests into the service, LSP responses
//! and published diagnostics out.

use futures::StreamExt as _;
use hird_lsp::Backend;
use serde_json::{Value, json};
use tower::{Service as _, ServiceExt as _};
use tower_lsp::jsonrpc::Request;
use tower_lsp::{ClientSocket, LspService};

/// A simple, well-typed Hirð file exercising every definition kind the
/// server resolves: effects, types, tools, functions, and actors.
const FIXTURE: &str = r#"effect Tool<t>
effect Spawn<t>

type Path = Path(String)

tool ReadFile : { path: Path } -> String

fn greet(name: String) -> String = name

fn read(p: Path) -> String ! {Tool<ReadFile>} = read_file({ path: p })

actor Echo {
  state: Int,

  message: EchoMsg = Ping,

  init: fn(start: Int) -> Int ! {} = start,

  handle Ping, n -> Int ! {} = n,
} ! {}

fn boot(n: Int) -> Pid<EchoMsg> ! {Spawn<EchoMsg>} = spawn(Echo, n)
"#;

/// The fixture's document URI.
const URI: &str = "file:///fixture.hird";

/// The LSP position of `target` within the first occurrence of `context`.
fn position(source: &str, context: &str, target: &str) -> Value {
    let context_start = source.find(context).expect("context not in source");
    let offset = context_start + context.find(target).expect("target not in context");
    let line = source[..offset].matches('\n').count();
    let line_start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    // The fixture is ASCII, so bytes and UTF-16 units coincide.
    json!({ "line": line, "character": offset - line_start })
}

/// The LSP range of `target` within the first occurrence of `context`.
fn range(source: &str, context: &str, target: &str) -> Value {
    let start = position(source, context, target);
    let end = json!({
        "line": start["line"],
        "character": start["character"].as_u64().unwrap() + target.len() as u64,
    });
    json!({ "start": start, "end": end })
}

/// Sends a request and returns its `result`, panicking on a JSON-RPC error.
async fn request(
    service: &mut LspService<Backend>,
    id: i64,
    method: &'static str,
    params: Value,
) -> Value {
    let request = Request::build(method).id(id).params(params).finish();
    let response = service
        .ready()
        .await
        .expect("service ready")
        .call(request)
        .await
        .expect("call succeeds")
        .expect("a request gets a response");
    let response = serde_json::to_value(response).expect("response serializes");
    assert!(
        response.get("error").is_none(),
        "unexpected JSON-RPC error: {response}"
    );
    response["result"].clone()
}

/// Sends a notification (no response expected).
async fn notify(service: &mut LspService<Backend>, method: &'static str, params: Value) {
    let request = Request::build(method).params(params).finish();
    let response = service
        .ready()
        .await
        .expect("service ready")
        .call(request)
        .await
        .expect("call succeeds");
    assert!(response.is_none(), "a notification gets no response");
}

/// Runs the `initialize`/`initialized` handshake.
async fn initialize(service: &mut LspService<Backend>) -> Value {
    let result = request(service, 1, "initialize", json!({ "capabilities": {} })).await;
    notify(service, "initialized", json!({})).await;
    result
}

/// Opens `text` as `uri` and returns the published diagnostics.
async fn open(
    service: &mut LspService<Backend>,
    socket: &mut ClientSocket,
    uri: &str,
    text: &str,
) -> Value {
    let params = json!({
        "textDocument": { "uri": uri, "languageId": "hird", "version": 1, "text": text }
    });
    let ((), published) = tokio::join!(
        notify(service, "textDocument/didOpen", params),
        next_diagnostics(socket)
    );
    published
}

/// The next `textDocument/publishDiagnostics` notification's params.
async fn next_diagnostics(socket: &mut ClientSocket) -> Value {
    let notification = socket.next().await.expect("socket open");
    assert_eq!(
        notification.method(),
        "textDocument/publishDiagnostics",
        "the only notification the server sends"
    );
    notification
        .params()
        .cloned()
        .expect("diagnostics have params")
}

#[tokio::test]
async fn initialize_advertises_capabilities() {
    let (mut service, _socket) = LspService::new(Backend::new);
    let result = initialize(&mut service).await;
    assert_eq!(result["serverInfo"]["name"], "hird-lsp");
    assert_eq!(result["capabilities"]["hoverProvider"], true);
    assert_eq!(result["capabilities"]["definitionProvider"], true);
}

#[tokio::test]
async fn open_publishes_no_diagnostics_for_a_clean_file() {
    let (mut service, mut socket) = LspService::new(Backend::new);
    initialize(&mut service).await;
    let published = open(&mut service, &mut socket, URI, FIXTURE).await;
    assert_eq!(published["uri"], URI);
    assert_eq!(published["diagnostics"], json!([]));
}

#[tokio::test]
async fn save_publishes_type_errors_with_spans() {
    let (mut service, mut socket) = LspService::new(Backend::new);
    initialize(&mut service).await;
    open(&mut service, &mut socket, URI, FIXTURE).await;

    // Edit the buffer into a type error; nothing publishes until save.
    let broken = "fn broken() -> Int = \"nope\"\n";
    notify(
        &mut service,
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": URI, "version": 2 },
            "contentChanges": [{ "text": broken }]
        }),
    )
    .await;
    let ((), published) = tokio::join!(
        notify(
            &mut service,
            "textDocument/didSave",
            json!({ "textDocument": { "uri": URI } }),
        ),
        next_diagnostics(&mut socket)
    );

    let diagnostics = published["diagnostics"].as_array().expect("array");
    assert_eq!(diagnostics.len(), 1, "one type error: {published}");
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic["severity"], 1, "an error");
    assert_eq!(diagnostic["code"], "C0001");
    assert!(
        diagnostic["message"]
            .as_str()
            .expect("message")
            .contains("type mismatch"),
        "unexpected message: {diagnostic}"
    );
    assert_eq!(diagnostic["range"], range(broken, "\"nope\"", "\"nope\""));
}

#[tokio::test]
async fn hover_shows_inferred_types_and_effect_rows() {
    let (mut service, mut socket) = LspService::new(Backend::new);
    initialize(&mut service).await;
    open(&mut service, &mut socket, URI, FIXTURE).await;

    // A parameter reference: plain inferred type.
    let hover = request(
        &mut service,
        2,
        "textDocument/hover",
        json!({
            "textDocument": { "uri": URI },
            "position": position(FIXTURE, "= name", "name"),
        }),
    )
    .await;
    assert_eq!(hover["contents"]["value"], "```hird\nname : String\n```");
    assert_eq!(hover["range"], range(FIXTURE, "= name", "name"));

    // A function declaration name: full type including the effect row.
    let hover = request(
        &mut service,
        3,
        "textDocument/hover",
        json!({
            "textDocument": { "uri": URI },
            "position": position(FIXTURE, "fn read(", "read"),
        }),
    )
    .await;
    assert_eq!(
        hover["contents"]["value"],
        "```hird\nread : Path \u{2192} String ! {Tool<ReadFile>}\n```"
    );
}

#[tokio::test]
async fn definitions_resolve_functions_types_actors_and_effects() {
    let (mut service, mut socket) = LspService::new(Backend::new);
    initialize(&mut service).await;
    open(&mut service, &mut socket, URI, FIXTURE).await;

    // (context of the reference, referenced name, context of its definition)
    let cases = [
        ("read_file({ path: p })", "read_file", "tool ReadFile"),
        ("spawn(Echo", "Echo", "actor Echo"),
        ("! {Tool<ReadFile>}", "Tool", "effect Tool<t>"),
        ("greet", "greet", "fn greet"),
    ];
    for (id, (reference_context, name, definition_context)) in cases.into_iter().enumerate() {
        let response = request(
            &mut service,
            10 + i64::try_from(id).expect("small id"),
            "textDocument/definition",
            json!({
                "textDocument": { "uri": URI },
                "position": position(FIXTURE, reference_context, name),
            }),
        )
        .await;
        // The definition token: for a tool, the reference is the generated
        // function name but the definition is the marker.
        let target = if name == "read_file" {
            "ReadFile"
        } else {
            name
        };
        assert_eq!(response["uri"], URI, "case `{name}`");
        assert_eq!(
            response["range"],
            range(FIXTURE, definition_context, target),
            "case `{name}`"
        );
    }

    // `Path` names both the type and its constructor: two definition sites.
    let response = request(
        &mut service,
        20,
        "textDocument/definition",
        json!({
            "textDocument": { "uri": URI },
            "position": position(FIXTURE, "(p: Path)", "Path"),
        }),
    )
    .await;
    let locations = response.as_array().expect("two definitions");
    assert_eq!(locations.len(), 2);
    assert_eq!(
        locations[0]["range"],
        range(FIXTURE, "type Path", "Path"),
        "the type declaration"
    );
    assert_eq!(
        locations[1]["range"],
        range(FIXTURE, "= Path(String)", "Path"),
        "the constructor"
    );
}

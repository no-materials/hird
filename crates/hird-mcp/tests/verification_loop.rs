// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The verification loop an authoring agent runs over the MCP tools, as one
//! scripted scenario: a module written with typical mistakes is corrected
//! from tool output alone — codes, positions, messages, and hints, never the
//! compiler's terminal rendering — until `check_file` reports it clean, and
//! the introspection tools then confirm it does what was intended. Every
//! step edits the file on disk, so each answer also proves the cache saw
//! the edit.

use std::path::PathBuf;

use hird_mcp::Server;
use serde_json::{Value, json};

/// The first draft: a supervised counter that logs increments through a
/// tool, with the mistakes an LLM typically makes — a return type on a
/// handler, a redundant built-in effect declaration, handler and `main`
/// rows that miss what their bodies perform, and a handler returning a
/// bare state instead of a `Next<State>` outcome.
const DRAFT: &str = "\
module Tally

effect Tool<t>

type alias LogArgs = { message: String }

tool Log : LogArgs \u{2192} ()

type TallyConfig = TallyConfig(Int)
type alias TallyState = { count: Int }

actor Tally {
  state: TallyState,

  message: TallyMsg =
    | Increment(Int)
    | GetCount(ReplyTo<Int>),

  init: fn(config: TallyConfig) ! {} =
    let TallyConfig(start) = config in { count: start },

  handle Increment(by), st \u{2192} TallyState ! {} =
    log({ message: \"increment\" });
    Continue({ count: st.count + by, ..st }),

  handle GetCount(reply_to), st ! {Send<Int>} =
    reply(reply_to, st.count);
    st,
} ! {Tool<Log>, Send<Int>}

fn initial_config() \u{2192} TallyConfig = TallyConfig(0)

supervisor TallySup {
  strategy: one_for_one,
  intensity: 5,
  period: 60,
  children: [
    { id: tally, actor: Tally, start_args: initial_config(), restart: permanent },
  ]
}

fn demo_log(args: LogArgs) \u{2192} () = ()

fn main() \u{2192} () ! {Install, Supervise, Send<TallyMsg>} =
  install { Tool<Log> \u{2192} demo_log } in
  supervise(TallySup);
  let tally = child(TallySup, tally) in
  send(tally, Increment(2));
  let total = request(tally, GetCount) in
  if total == 2 then () else crash!(\"lost an increment\")
";

/// Calls one tool and returns its `structuredContent`, asserting success.
fn call(server: &mut Server, tool: &str, arguments: Value) -> Value {
    let message = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    });
    let response = server
        .handle_message(&message.to_string())
        .expect("a request gets a response");
    let response: Value = serde_json::from_str(&response).expect("a JSON response");
    let result = &response["result"];
    assert!(
        result.get("isError").is_none_or(|e| e == false),
        "tool `{tool}` failed: {result}"
    );
    result["structuredContent"].clone()
}

/// The module under authorship: its path and the lines on disk.
struct Draft {
    path: String,
    lines: Vec<String>,
}

impl Draft {
    /// Writes the first draft to a directory of its own.
    fn new() -> Self {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("verification_loop");
        std::fs::create_dir_all(&dir).expect("the fixture dir");
        let path = dir.join("tally.hird");
        std::fs::write(&path, DRAFT).expect("the draft writes");
        Self {
            path: path.to_str().expect("a UTF-8 path").to_owned(),
            lines: DRAFT.lines().map(String::from).collect(),
        }
    }

    /// Writes the current lines to disk — the edit the next tool call must
    /// see.
    fn save(&self) {
        let mut text = self.lines.join("\n");
        text.push('\n');
        std::fs::write(&self.path, text).expect("the draft writes");
    }

    /// The 1-based line `line` of the draft, mutably.
    fn line(&mut self, line: &Value) -> &mut String {
        let index = usize::try_from(line.as_u64().expect("a line")).expect("a small line") - 1;
        &mut self.lines[index]
    }

    /// `check_file` on the draft: `ok` and the diagnostics.
    fn check(&self, server: &mut Server) -> (bool, Vec<Value>) {
        let result = call(server, "check_file", json!({ "file": self.path }));
        (
            result["ok"] == true,
            result["diagnostics"]
                .as_array()
                .expect("diagnostics")
                .clone(),
        )
    }
}

/// The byte index of 1-based character `column` in `line`.
fn byte_of(line: &str, column: &Value) -> usize {
    let column = usize::try_from(column.as_u64().expect("a column")).expect("a small column");
    line.char_indices()
        .nth(column - 1)
        .map_or(line.len(), |(i, _)| i)
}

/// Replaces the effect row written after `!` on `line` with `row`.
fn rewrite_row(line: &mut String, row: &str) {
    let start = line.find("! {").expect("a declared row") + 2;
    let end = start + line[start..].find('}').expect("a closed row") + 1;
    line.replace_range(start..end, row);
}

/// The `declared … but body performs …` rows a C0030 message names.
fn performed_row(message: &Value) -> &str {
    message
        .as_str()
        .expect("a message")
        .split_once(" but body performs ")
        .expect("a C0030 message")
        .1
}

#[test]
fn an_agent_corrects_a_module_from_tool_output_alone() {
    let mut server = Server::new();
    let mut draft = Draft::new();

    // Round 1: the file does not parse. One diagnostic, with the code, the
    // position of the offending token, and a hint saying what to remove.
    let (ok, diagnostics) = draft.check(&mut server);
    assert!(!ok);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let d = &diagnostics[0];
    assert_eq!(d["code"], "P0006");
    assert_eq!(d["file"], draft.path);
    assert!(
        d["help"]
            .as_str()
            .is_some_and(|help| help.contains("remove")),
        "{d}"
    );
    // Drop everything from the reported column up to the row marker.
    let line = draft.line(&d["line"]);
    let arrow = byte_of(line, &d["column"]);
    let bang = arrow + line[arrow..].find('!').expect("a row follows");
    line.replace_range(arrow..bang, "");
    draft.save();

    // Round 2: the file parses and the checker reports every declaration's
    // problem at once — a warning, two row mismatches, a wrong outcome type,
    // and the actor summary the handler mismatch drags along.
    let (ok, diagnostics) = draft.check(&mut server);
    assert!(!ok);
    let codes: Vec<&str> = diagnostics
        .iter()
        .map(|d| d["code"].as_str().expect("a code"))
        .collect();
    assert_eq!(codes, ["C0056", "C0038", "C0030", "C0001", "C0030"]);
    assert_eq!(diagnostics[0]["severity"], "Warning");
    assert!(diagnostics[1..].iter().all(|d| d["severity"] == "Error"));

    // Fix bottom-up so earlier line numbers stay valid while a line is
    // removed. C0038 needs no edit of its own: the actor summary is right,
    // the handler row under it is wrong, and C0030 says so.
    for d in diagnostics.iter().rev() {
        match d["code"].as_str().expect("a code") {
            // The row the body actually performs is in the message.
            "C0030" => {
                let row = performed_row(&d["message"]).to_owned();
                rewrite_row(draft.line(&d["line"]), &row);
            }
            // `expected Next<…>`: wrap the handler's final expression.
            "C0001" => {
                assert!(
                    d["message"]
                        .as_str()
                        .is_some_and(|m| m.contains("expected `Next<")),
                    "{d}"
                );
                let last = draft.line(&d["end_line"]);
                let expr = last.trim().trim_end_matches(',');
                *last = format!("    Continue({expr}),");
            }
            // "redundant and can be removed": remove it.
            "C0056" => {
                let index = usize::try_from(d["line"].as_u64().expect("a line")).unwrap() - 1;
                draft.lines.remove(index);
            }
            "C0038" => {}
            other => panic!("unexpected diagnostic {other}: {d}"),
        }
    }
    draft.save();

    // Round 3: clean, warnings included.
    let (ok, diagnostics) = draft.check(&mut server);
    assert!(ok, "{diagnostics:#?}");
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");

    // Clean is not the same as right: the introspection tools confirm the
    // module does what the author meant.
    let protocol = call(
        &mut server,
        "explain_actor_protocol",
        json!({ "file": draft.path, "actor_name": "Tally" }),
    );
    let handlers: Vec<&str> = protocol["actor"]["handlers"]
        .as_array()
        .expect("handlers")
        .iter()
        .map(|h| h["message"].as_str().expect("a message"))
        .collect();
    assert_eq!(handlers, ["Increment", "GetCount"]);
    assert_eq!(
        protocol["actor"]["effects"]["display"],
        "{Send<Int>, Tool<Log>}"
    );

    let graph = call(
        &mut server,
        "emit_actor_effect_graph",
        json!({ "file": draft.path, "actor_name": "Tally" }),
    );
    let names = |nodes: &Value| -> Vec<String> {
        nodes
            .as_array()
            .expect("nodes")
            .iter()
            .map(|n| n["name"].as_str().expect("a name").to_owned())
            .collect()
    };
    assert_eq!(names(&graph["actors"]), ["Tally"]);
    assert_eq!(names(&graph["supervisors"]), ["TallySup"]);
    assert_eq!(names(&graph["tools"]), ["Log"]);

    let main = call(
        &mut server,
        "explain_effect_row",
        json!({ "file": draft.path, "fn_name": "main" }),
    );
    assert_eq!(
        main["effect_row"],
        "{Await<Int>, Install, Send<TallyMsg>, Supervise}"
    );
    assert_eq!(main["pure"], false);
    assert_eq!(main["open"], false);
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The MCP protocol layer: JSON-RPC 2.0 message handling over any
//! line-oriented transport.
//!
//! [`Server::handle_message`] maps one incoming message to at most one
//! response line, so the binary (and the tests) drive the server without an
//! async runtime. Requests get responses; notifications get none. Tool
//! failures come back as `isError` tool results per the MCP spec — JSON-RPC
//! errors are reserved for protocol misuse (unknown method, malformed
//! message, unknown tool).

use serde_json::{Value, json};

use crate::analysis::Cache;
use crate::tools;

/// The protocol revisions this server accepts; the last is its default.
const PROTOCOL_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];

/// A stateful MCP server: one per client connection.
#[derive(Debug, Default)]
pub struct Server {
    /// The per-file compilation cache behind every tool.
    cache: Cache,
}

impl Server {
    /// A server with an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles one JSON-RPC message, returning the serialized response for a
    /// request and `None` for a notification.
    pub fn handle_message(&mut self, message: &str) -> Option<String> {
        let Ok(message) = serde_json::from_str::<Value>(message) else {
            return Some(error_response(Value::Null, -32700, "parse error"));
        };
        let method = message.get("method").and_then(Value::as_str)?;
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(json!({}));
        // A notification: handle side effects (none in v0.1), respond never.
        let id = id?;
        let response = match method {
            "initialize" => Ok(initialize_result(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::descriptors() })),
            "tools/call" => self.call_tool(&params),
            _ => Err((-32601, format!("method `{method}` not found"))),
        };
        Some(match response {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string(),
            Err((code, message)) => error_response(id, code, &message),
        })
    }

    /// Handles `tools/call`: dispatches the named tool and wraps its result
    /// (or its structured error) as MCP tool-call content.
    fn call_tool(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err((-32602, String::from("missing tool `name`")));
        };
        if !tools::is_known(name) {
            return Err((-32602, format!("unknown tool `{name}`")));
        }
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        Ok(match tools::call(&mut self.cache, name, &arguments) {
            Ok(value) => json!({
                "content": [{ "type": "text", "text": pretty(&value) }],
                "structuredContent": value,
            }),
            Err(error) => {
                let value = error.to_value();
                json!({
                    "content": [{ "type": "text", "text": pretty(&value) }],
                    "structuredContent": value,
                    "isError": true,
                })
            }
        })
    }
}

/// The `initialize` result: negotiated protocol version, capabilities, and
/// server identity.
fn initialize_result(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|v| PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(PROTOCOL_VERSIONS[PROTOCOL_VERSIONS.len() - 1]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "hird-mcp",
            "title": "Hirð compiler introspection",
            "version": env!("CARGO_PKG_VERSION"),
        },
    })
}

/// A serialized JSON-RPC error response.
fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

/// `value` as indented JSON, for the human-readable content half of a tool
/// result.
fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The documents the server serves as MCP resources: the agent-facing
//! writing guide, the parser diagnostic code index, and the phrasebook,
//! embedded at build time so the binary carries them wherever it is
//! installed.

use serde_json::{Value, json};

/// One embedded document.
struct Resource {
    /// The `hird://` URI clients read it by.
    uri: &'static str,
    /// The programmatic name.
    name: &'static str,
    /// The human-readable title.
    title: &'static str,
    /// What the document is for, for a client choosing what to read.
    description: &'static str,
    /// The document's text.
    text: &'static str,
}

/// The MIME type every resource shares.
const MIME_TYPE: &str = "text/markdown";

/// The served documents, in the order a client should read them.
const RESOURCES: [Resource; 3] = [
    Resource {
        uri: "hird://docs/writing-hird-llm",
        name: "writing-hird-llm",
        title: "Writing Hirð for LLM agents",
        description: "The constraints the compiler enforces, the tools to query it instead of \
                      guessing, the verification loop over this server, and the mistakes it \
                      most often catches in generated code. Read it before writing Hirð.",
        text: include_str!("../../../docs/writing-hird-llm.md"),
    },
    Resource {
        uri: "hird://docs/parser-diagnostics",
        name: "parser-diagnostics",
        title: "Parser diagnostic codes",
        description: "Every `P…` code a parse diagnostic can carry, with its meaning. Checker \
                      `C…` codes are explained by their own messages and by the mistakes table \
                      in the writing guide.",
        text: include_str!("../../../docs/parser-diagnostics.md"),
    },
    Resource {
        uri: "hird://phrasebook",
        name: "phrasebook",
        title: "Hirð phrasebook",
        description: "The canonical surface syntax, one compiling snippet per construct; the \
                      dense reference (~3k tokens) to keep in context while writing.",
        text: include_str!("../../../phrasebook.md"),
    },
];

/// The `resources/list` result: every served document's descriptor.
pub(crate) fn list() -> Value {
    let resources: Vec<Value> = RESOURCES
        .iter()
        .map(|r| {
            json!({
                "uri": r.uri,
                "name": r.name,
                "title": r.title,
                "description": r.description,
                "mimeType": MIME_TYPE,
            })
        })
        .collect();
    json!({ "resources": resources })
}

/// The `resources/read` result for `uri`, or `None` for a URI not served.
pub(crate) fn read(uri: &str) -> Option<Value> {
    RESOURCES.iter().find(|r| r.uri == uri).map(|r| {
        json!({
            "contents": [{ "uri": r.uri, "mimeType": MIME_TYPE, "text": r.text }],
        })
    })
}

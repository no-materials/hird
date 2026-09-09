// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The one prompt the server serves: author a supervised actor module and
//! verify it through the tools until the compiler confirms it.

use serde_json::{Value, json};

/// The prompt's name.
const NAME: &str = "author_supervised_module";

/// The prompt's one-line description.
const DESCRIPTION: &str = "Write a new Hirð module with a supervised actor and verify it \
    against the compiler through the hird tools until `check_file` reports it clean and the \
    introspection tools confirm its intent.";

/// The `prompts/list` result: the one prompt and its arguments.
pub(crate) fn list() -> Value {
    json!({
        "prompts": [{
            "name": NAME,
            "title": "Author a supervised actor module",
            "description": DESCRIPTION,
            "arguments": [
                {
                    "name": "file",
                    "description": "Path of the .hird file to write, absolute or relative to \
                                    the server's working directory.",
                    "required": true,
                },
                {
                    "name": "purpose",
                    "description": "What the module should do: the actor's job, the messages \
                                    it handles, the tools it calls.",
                    "required": true,
                },
            ],
        }],
    })
}

/// The `prompts/get` result for prompt `name` with `arguments`, or a
/// JSON-RPC error (code, message) for an unknown prompt or a missing
/// argument.
pub(crate) fn get(name: &str, arguments: &Value) -> Result<Value, (i64, String)> {
    if name != NAME {
        return Err((-32602, format!("unknown prompt `{name}`")));
    }
    let argument = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| (-32602, format!("missing prompt argument `{key}`")))
    };
    let file = argument("file")?;
    let purpose = argument("purpose")?;
    let text = format!(
        "Write a Hirð module at `{file}` that {purpose}.\n\
         \n\
         Before writing, read the resources `hird://docs/writing-hird-llm` (the constraints \
         the compiler enforces and the mistakes it most often catches) and `hird://phrasebook` \
         (the canonical surface syntax; every snippet in it compiles).\n\
         \n\
         The module declares one actor with a sum-type mailbox, a supervisor that owns it, and \
         a `main` that starts the tree with `supervise`, reaches the child with `child`, and \
         drives one round. Give every function and handler exactly the effect row its body \
         performs. If the actor calls tools, `main` resolves them through an `install` block.\n\
         \n\
         Then verify from tool output alone:\n\
         1. Call `check_file` on the file. Fix each diagnostic by its code, position, and \
         `help`; a cascade usually collapses after one fix, so re-check before touching the \
         next. Repeat until `ok` is true with no diagnostics, warnings included.\n\
         2. Confirm the intent: `list_definitions` for the outline, `explain_actor_protocol` \
         for the handlers and the actor's row, `emit_actor_effect_graph` for the supervisor \
         and the tools the actor reaches, `explain_effect_row` for `main`.\n\
         3. Report what the module does in terms of what those tools returned."
    );
    Ok(json!({
        "description": DESCRIPTION,
        "messages": [{ "role": "user", "content": { "type": "text", "text": text } }],
    }))
}

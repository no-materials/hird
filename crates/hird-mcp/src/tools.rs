// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The introspection tools the server exposes, and their dispatch.
//!
//! Every tool takes a `file` argument, compiles it through [`Cache`], and
//! answers from the compiled artifacts: the checker's side tables, the
//! lowered IR, and the actor/effect graph. Failures are [`ToolError`]s — a
//! stable machine code, a message, and optional structured data — which the
//! server renders as `isError` tool results, never as protocol errors.

use std::collections::BTreeSet;

use hird_check::NodeKey;
use hird_ir::{ActorNode, EFFECT_GRAPH_SCHEMA_VERSION, EffectRowRef, IrDecl, IrExpr};
use hird_types::{EffectRow, Type};
use serde_json::{Value, json};

use crate::analysis::{Analysis, Cache, offset_of, tool_fn_name};

/// A failed tool call: a stable code, a message, and optional details.
#[derive(Debug)]
pub(crate) struct ToolError {
    /// The machine-readable error code (`file_not_found`, `parse_error`,
    /// `check_error`, `not_found`, `invalid_params`, …).
    pub(crate) code: &'static str,
    /// The human-readable message.
    pub(crate) message: String,
    /// Structured details (diagnostics, available names), when useful.
    pub(crate) data: Option<Value>,
}

impl ToolError {
    /// A tool error with no extra data.
    pub(crate) fn new(code: &'static str, message: String) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }

    /// A tool error carrying structured details.
    pub(crate) fn with_data(code: &'static str, message: String, data: Value) -> Self {
        Self {
            code,
            message,
            data: Some(data),
        }
    }

    /// The error as the structured payload of an `isError` tool result.
    pub(crate) fn to_value(&self) -> Value {
        let mut error = json!({ "code": self.code, "message": self.message });
        if let Some(data) = &self.data {
            error["data"] = data.clone();
        }
        json!({ "error": error })
    }
}

/// The default `get_context_for_symbol` token budget.
const DEFAULT_BUDGET: usize = 400;

/// The tool descriptors served by `tools/list`.
pub(crate) fn descriptors() -> Value {
    let file = json!({ "type": "string", "description": "Path to a .hird source file." });
    json!([
        {
            "name": "infer_type",
            "description": "Infer the type and effect row of the expression at a source \
                            location (1-based line, 1-based character column) in a .hird file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": file,
                    "line": { "type": "integer", "description": "1-based source line." },
                    "column": { "type": "integer", "description": "1-based character column." },
                },
                "required": ["file", "line", "column"],
            },
        },
        {
            "name": "lookup_definition",
            "description": "Look up a top-level definition by name: source location, type, \
                            doc comment, and kind.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": file,
                    "name": { "type": "string", "description": "The definition's name." },
                },
                "required": ["file", "name"],
            },
        },
        {
            "name": "explain_effect_row",
            "description": "Explain a function's effect row: the canonical row plus a \
                            human-readable explanation of each effect.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": file,
                    "fn_name": { "type": "string", "description": "The function's name." },
                },
                "required": ["file", "fn_name"],
            },
        },
        {
            "name": "render_ir_fragment",
            "description": "Render the typed IR of one top-level definition as JSON.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": file,
                    "name": { "type": "string", "description": "The definition's name." },
                },
                "required": ["file", "name"],
            },
        },
        {
            "name": "explain_actor_protocol",
            "description": "Describe an actor's protocol: message constructors, state type, \
                            init, handler signatures, and the declared effect summary.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": file,
                    "actor_name": { "type": "string", "description": "The actor's name." },
                },
                "required": ["file", "actor_name"],
            },
        },
        {
            "name": "emit_actor_effect_graph",
            "description": "Emit the actor/effect graph rooted at an actor: reachable actors \
                            (via Send/Await/Spawn message types), supervisor relationships, \
                            and transitive tool effects.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": file,
                    "actor_name": { "type": "string", "description": "The root actor's name." },
                },
                "required": ["file", "actor_name"],
            },
        },
        {
            "name": "get_context_for_symbol",
            "description": "A token-budget-aware summary of a symbol: kind, signature, effect \
                            row, doc, callers, and callees, fitted to the budget.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": file,
                    "name": { "type": "string", "description": "The symbol's name." },
                    "budget": {
                        "type": "integer",
                        "description": "Approximate token budget for the summary (default 400).",
                    },
                },
                "required": ["file", "name"],
            },
        },
        {
            "name": "get_context_budget",
            "description": "Approximate token costs of including the file's types, effects, \
                            actors, supervisors, tools, and function signatures in an LLM \
                            context window.",
            "inputSchema": {
                "type": "object",
                "properties": { "file": file },
                "required": ["file"],
            },
        },
    ])
}

/// Whether `tool` is one of the tools this server serves.
pub(crate) fn is_known(tool: &str) -> bool {
    matches!(
        tool,
        "infer_type"
            | "lookup_definition"
            | "explain_effect_row"
            | "render_ir_fragment"
            | "explain_actor_protocol"
            | "emit_actor_effect_graph"
            | "get_context_for_symbol"
            | "get_context_budget"
    )
}

/// Dispatches one tool call.
pub(crate) fn call(cache: &mut Cache, tool: &str, args: &Value) -> Result<Value, ToolError> {
    let analysis = cache.analysis(str_arg(args, "file")?)?;
    match tool {
        "infer_type" => infer_type(analysis, args),
        "lookup_definition" => lookup_definition(analysis, args),
        "explain_effect_row" => explain_effect_row(analysis, args),
        "render_ir_fragment" => render_ir_fragment(analysis, args),
        "explain_actor_protocol" => explain_actor_protocol(analysis, args),
        "emit_actor_effect_graph" => emit_actor_effect_graph(analysis, args),
        "get_context_for_symbol" => get_context_for_symbol(analysis, args),
        "get_context_budget" => Ok(get_context_budget(analysis)),
        _ => Err(ToolError::new(
            "invalid_params",
            format!("unknown tool `{tool}`"),
        )),
    }
}

// ── the tools ────────────────────────────────────────────────────

/// `infer_type(file, line, column)` — the inferred type (and effect row, for
/// function-typed expressions) at a source location.
fn infer_type(analysis: &Analysis, args: &Value) -> Result<Value, ToolError> {
    let line = usize_arg(args, "line")?;
    let column = usize_arg(args, "column")?;
    let offset = offset_of(&analysis.source, line, column).ok_or_else(|| {
        ToolError::new(
            "invalid_params",
            format!("{line}:{column} is outside `{}`", analysis.file),
        )
    })?;
    let token = analysis
        .token_at(offset)
        .ok_or_else(|| ToolError::new("not_found", format!("no token at {line}:{column}")))?;
    let ty = analysis
        .checked
        .type_at(NodeKey::of_token(&token))
        .or_else(|| {
            token
                .ancestors()
                .find_map(|node| analysis.checked.type_at(NodeKey::of_node(node)))
        })
        .or_else(|| {
            (token.kind() == hird_parse::SyntaxKind::IDENT)
                .then(|| analysis.checked.bindings.get(token.text()))
                .flatten()
        })
        .ok_or_else(|| {
            ToolError::new(
                "not_found",
                format!("no typed expression at {line}:{column}"),
            )
        })?
        .normalized();
    let effect_row = fn_row(&ty).map_or_else(|| String::from("{}"), |row| format!("{row}"));
    Ok(json!({
        "file": analysis.file,
        "line": line,
        "column": column,
        "token": token.text(),
        "type": format!("{ty}"),
        "effect_row": effect_row,
    }))
}

/// `lookup_definition(file, name)` — location, type, doc, and kind of a
/// top-level definition.
fn lookup_definition(analysis: &Analysis, args: &Value) -> Result<Value, ToolError> {
    let name = str_arg(args, "name")?;
    let definition = analysis
        .definitions
        .iter()
        .find(|d| d.name == name)
        .ok_or_else(|| not_found(analysis, name))?;
    let ty = analysis
        .checked
        .bindings
        .get(name)
        .or_else(|| {
            (definition.kind == "tool")
                .then(|| analysis.checked.bindings.get(&tool_fn_name(name)))
                .flatten()
        })
        .map(|ty| format!("{}", ty.normalized()));
    Ok(json!({
        "file": analysis.file,
        "name": definition.name,
        "kind": definition.kind,
        "line": definition.line,
        "type": ty,
        "doc": definition.doc,
    }))
}

/// `explain_effect_row(file, fn_name)` — a function's effect row with each
/// effect explained.
fn explain_effect_row(analysis: &Analysis, args: &Value) -> Result<Value, ToolError> {
    let name = str_arg(args, "fn_name")?;
    let ty = analysis
        .checked
        .bindings
        .get(name)
        .ok_or_else(|| not_found(analysis, name))?
        .normalized();
    let row = fn_row(&ty).ok_or_else(|| {
        ToolError::new(
            "not_a_function",
            format!("`{name}` has type `{ty}`, which is not a function type"),
        )
    })?;
    let effects: Vec<Value> = row
        .effects()
        .map(|effect| {
            json!({
                "effect": format!("{effect}"),
                "explanation": explain_effect(
                    &format!("{}", effect.head()),
                    effect.args().first().map(|arg| format!("{arg}")),
                ),
            })
        })
        .collect();
    let open = row.tail().is_some();
    Ok(json!({
        "file": analysis.file,
        "name": name,
        "type": format!("{ty}"),
        "effect_row": format!("{row}"),
        "open": open,
        "pure": effects.is_empty() && !open,
        "effects": effects,
    }))
}

/// `render_ir_fragment(file, name)` — the typed IR of one definition.
fn render_ir_fragment(analysis: &Analysis, args: &Value) -> Result<Value, ToolError> {
    let name = str_arg(args, "name")?;
    let decl = find_decl(analysis, name).ok_or_else(|| not_found(analysis, name))?;
    let ir = serde_json::to_value(decl)
        .map_err(|e| ToolError::new("internal", format!("cannot serialize IR: {e}")))?;
    Ok(json!({
        "module": analysis.ir.name,
        "name": decl_name(decl),
        "ir": ir,
    }))
}

/// `explain_actor_protocol(file, actor_name)` — an actor's mailbox, state,
/// init, handlers, and effect summary.
fn explain_actor_protocol(analysis: &Analysis, args: &Value) -> Result<Value, ToolError> {
    let name = str_arg(args, "actor_name")?;
    let actor = find_actor(analysis, name)?;
    let actor = serde_json::to_value(actor)
        .map_err(|e| ToolError::new("internal", format!("cannot serialize actor: {e}")))?;
    Ok(json!({
        "module": analysis.graph.module,
        "file": analysis.file,
        "actor": actor,
    }))
}

/// `emit_actor_effect_graph(file, actor_name)` — the subgraph reachable from
/// one actor: actors (following `Send`/`Await`/`Spawn` message types),
/// supervisors of included actors (and their whole child sets), and every
/// tool named by an included actor's effect summary.
fn emit_actor_effect_graph(analysis: &Analysis, args: &Value) -> Result<Value, ToolError> {
    let root = find_actor(analysis, str_arg(args, "actor_name")?)?;
    let graph = &analysis.graph;

    let mut actors: BTreeSet<&str> = BTreeSet::from([root.name.as_str()]);
    let mut supervisors: BTreeSet<&str> = BTreeSet::new();
    let mut tools: BTreeSet<&str> = BTreeSet::new();
    loop {
        let before = (actors.len(), supervisors.len(), tools.len());
        let current: Vec<&ActorNode> = graph
            .actors
            .iter()
            .filter(|a| actors.contains(a.name.as_str()))
            .collect();
        for actor in current {
            for effect in &actor.effects.effects {
                let arg = effect.args.first().map(|t| t.display.as_str());
                match effect.head.as_str() {
                    "Tool" => {
                        if let Some(tool) =
                            graph.tools.iter().find(|t| Some(t.name.as_str()) == arg)
                        {
                            tools.insert(&tool.name);
                        }
                    }
                    "Send" | "Await" | "Spawn" => {
                        if let Some(target) = graph
                            .actors
                            .iter()
                            .find(|a| Some(a.message.name.as_str()) == arg)
                        {
                            actors.insert(&target.name);
                        }
                    }
                    _ => {}
                }
            }
        }
        for sup in &graph.supervisors {
            if sup
                .children
                .iter()
                .any(|c| actors.contains(c.actor.as_str()))
            {
                supervisors.insert(&sup.name);
                for child in &sup.children {
                    if graph.actors.iter().any(|a| a.name == child.actor) {
                        actors.insert(child.actor.as_str());
                    }
                }
            }
        }
        if before == (actors.len(), supervisors.len(), tools.len()) {
            break;
        }
    }

    let included_actors: Vec<&ActorNode> = graph
        .actors
        .iter()
        .filter(|a| actors.contains(a.name.as_str()))
        .collect();
    let included_supervisors: Vec<_> = graph
        .supervisors
        .iter()
        .filter(|s| supervisors.contains(s.name.as_str()))
        .collect();
    let included_tools: Vec<_> = graph
        .tools
        .iter()
        .filter(|t| tools.contains(t.name.as_str()))
        .collect();
    Ok(json!({
        "schema_version": EFFECT_GRAPH_SCHEMA_VERSION,
        "module": graph.module,
        "root": root.name,
        "actors": included_actors,
        "supervisors": included_supervisors,
        "tools": included_tools,
    }))
}

/// `get_context_for_symbol(file, name, budget)` — a prompt-ready summary of
/// a symbol, assembled section by section (signature, effects, doc, callers,
/// callees) while it fits the token budget.
fn get_context_for_symbol(analysis: &Analysis, args: &Value) -> Result<Value, ToolError> {
    let name = str_arg(args, "name")?;
    let budget = match args.get("budget") {
        Some(_) => usize_arg(args, "budget")?,
        None => DEFAULT_BUDGET,
    };
    let decl = find_decl(analysis, name).ok_or_else(|| not_found(analysis, name))?;
    let decl_name = decl_name(decl);
    let kind = decl_kind(decl);

    let mut sections: Vec<(&str, String)> = vec![("signature", header_line(analysis, decl))];
    if let Some(row) = effects_line(analysis, decl) {
        sections.push(("effects", row));
    }
    if let Some(doc) = analysis
        .definitions
        .iter()
        .find(|d| d.name == decl_name && d.doc.is_some())
        .and_then(|d| d.doc.clone())
    {
        sections.push(("doc", format!("doc: {doc}")));
    }
    let (callers, callees) = call_graph(analysis, decl);
    if !callers.is_empty() {
        sections.push(("callers", format!("callers: {}", callers.join(", "))));
    }
    if !callees.is_empty() {
        sections.push(("callees", format!("callees: {}", callees.join(", "))));
    }

    let mut summary = String::new();
    let mut omitted: Vec<&str> = Vec::new();
    for (section, text) in &sections {
        let text = if summary.is_empty() && estimate_tokens(text) > budget {
            // The signature is the one section always worth truncating into
            // the budget rather than dropping.
            truncate_to(text, budget)
        } else {
            text.clone()
        };
        let joined = if summary.is_empty() {
            text
        } else {
            format!("{summary}\n{text}")
        };
        if !summary.is_empty() && estimate_tokens(&joined) > budget {
            omitted.push(section);
            continue;
        }
        summary = joined;
    }

    Ok(json!({
        "file": analysis.file,
        "symbol": decl_name,
        "kind": kind,
        "budget": budget,
        "approx_tokens": estimate_tokens(&summary),
        "summary": summary,
        "omitted": omitted,
    }))
}

/// `get_context_budget(file)` — approximate per-category token costs of the
/// file's declarations, at ~4 characters per token.
fn get_context_budget(analysis: &Analysis) -> Value {
    let graph = &analysis.graph;
    let mut types = 0;
    let mut functions = 0;
    let mut effects: BTreeSet<String> = BTreeSet::new();
    for decl in &analysis.ir.declarations {
        match decl {
            IrDecl::Type(_) => types += estimate_tokens(&header_line(analysis, decl)),
            IrDecl::Fn(_) | IrDecl::Extern(_) => {
                functions += estimate_tokens(&header_line(analysis, decl));
            }
            IrDecl::Tool(_) | IrDecl::Actor(_) | IrDecl::Supervisor(_) => {}
        }
        if let Some(ty) = analysis.checked.bindings.get(decl_name(decl)) {
            let ty = ty.normalized();
            if let Some(row) = fn_row(&ty) {
                effects.extend(row.effects().map(|e| format!("{e}")));
            }
        }
    }
    let actors: usize = graph
        .actors
        .iter()
        .map(|actor| {
            effects.extend(actor.effects.effects.iter().map(|e| e.display.clone()));
            estimate_tokens(&actor_header(actor))
                + estimate_row(&actor.effects)
                + actor
                    .handlers
                    .iter()
                    .map(|h| estimate_tokens(&h.message) + estimate_row(&h.effects))
                    .sum::<usize>()
        })
        .sum();
    let supervisors: usize = graph
        .supervisors
        .iter()
        .map(|sup| {
            estimate_tokens(&format!(
                "supervisor {} — {}, intensity {}/{}s, children: {}",
                sup.name,
                sup.strategy,
                sup.intensity,
                sup.period,
                sup.children
                    .iter()
                    .map(|c| format!("{}: {} ({})", c.id, c.actor, c.restart))
                    .collect::<Vec<_>>()
                    .join(", "),
            ))
        })
        .sum();
    let tools: usize = graph
        .tools
        .iter()
        .map(|tool| {
            effects.insert(format!("Tool<{}>", tool.name));
            effects.extend(tool.effects.effects.iter().map(|e| e.display.clone()));
            estimate_tokens(&format!(
                "tool {} : {} → {}",
                tool.name, tool.input.display, tool.output.display
            ))
        })
        .sum();
    let effects: usize = effects.iter().map(|e| estimate_tokens(e)).sum();
    let total = types + effects + actors + supervisors + tools + functions;
    json!({
        "file": analysis.file,
        "module": analysis.ir.name,
        "approx_tokens": {
            "types": types,
            "effects": effects,
            "actors": actors,
            "supervisors": supervisors,
            "tools": tools,
            "functions": functions,
            "total": total,
        },
        "note": "approximate, at ~4 characters per token",
    })
}

// ── shared helpers ───────────────────────────────────────────────

/// The required string argument `name` of `args`.
fn str_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, ToolError> {
    args.get(name).and_then(Value::as_str).ok_or_else(|| {
        ToolError::new(
            "invalid_params",
            format!("missing or non-string argument `{name}`"),
        )
    })
}

/// The required non-negative integer argument `name` of `args`.
fn usize_arg(args: &Value, name: &str) -> Result<usize, ToolError> {
    args.get(name)
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| {
            ToolError::new(
                "invalid_params",
                format!("missing or non-integer argument `{name}`"),
            )
        })
}

/// A `not_found` error for `name`, listing the file's definition names.
fn not_found(analysis: &Analysis, name: &str) -> ToolError {
    let available: BTreeSet<&str> = analysis
        .definitions
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    ToolError::with_data(
        "not_found",
        format!("`{name}` is not defined in `{}`", analysis.file),
        json!({ "available": available }),
    )
}

/// The effect row of a (possibly quantified) function type.
fn fn_row(ty: &Type) -> Option<&EffectRow> {
    match ty {
        Type::TyFn(_, _, row) => Some(row),
        Type::TyForall(_, _, body) => fn_row(body),
        _ => None,
    }
}

/// The top-level IR declaration named `name`; a tool also resolves through
/// its generated function name.
fn find_decl<'a>(analysis: &'a Analysis, name: &str) -> Option<&'a IrDecl> {
    analysis.ir.declarations.iter().find(|decl| {
        decl_name(decl) == name
            || matches!(decl, IrDecl::Tool(tool) if tool_fn_name(&tool.name) == name)
    })
}

/// The actor node named `name`, or a `not_found` error listing the actors.
fn find_actor<'a>(analysis: &'a Analysis, name: &str) -> Result<&'a ActorNode, ToolError> {
    analysis
        .graph
        .actors
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| {
            let available: Vec<&str> = analysis
                .graph
                .actors
                .iter()
                .map(|a| a.name.as_str())
                .collect();
            ToolError::with_data(
                "not_found",
                format!("`{name}` is not an actor in `{}`", analysis.file),
                json!({ "available_actors": available }),
            )
        })
}

/// The name a declaration binds.
fn decl_name(decl: &IrDecl) -> &str {
    match decl {
        IrDecl::Fn(d) => &d.name,
        IrDecl::Type(d) => &d.name,
        IrDecl::Extern(d) => &d.name,
        IrDecl::Tool(d) => &d.name,
        IrDecl::Actor(d) => &d.name,
        IrDecl::Supervisor(d) => &d.name,
    }
}

/// The kind label of a declaration.
fn decl_kind(decl: &IrDecl) -> &'static str {
    match decl {
        IrDecl::Fn(_) => "function",
        IrDecl::Type(_) => "type",
        IrDecl::Extern(_) => "extern",
        IrDecl::Tool(_) => "tool",
        IrDecl::Actor(_) => "actor",
        IrDecl::Supervisor(_) => "supervisor",
    }
}

/// The one-line signature of a declaration, for summaries and budgets.
fn header_line(analysis: &Analysis, decl: &IrDecl) -> String {
    match decl {
        IrDecl::Fn(d) => format!("fn {} : {}", d.name, binding_type(analysis, &d.name)),
        IrDecl::Extern(d) => format!("extern {} : {}", d.name, d.ty.normalized()),
        IrDecl::Type(d) => {
            let params = if d.params.is_empty() {
                String::new()
            } else {
                format!("<{}>", d.params.join(", "))
            };
            let constructors = d
                .constructors
                .iter()
                .map(|c| {
                    if c.fields.is_empty() {
                        c.name.clone()
                    } else {
                        format!(
                            "{}({})",
                            c.name,
                            c.fields
                                .iter()
                                .map(|f| format!("{f}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join(" | ");
            format!("type {}{params} = {constructors}", d.name)
        }
        IrDecl::Tool(d) => analysis
            .graph
            .tools
            .iter()
            .find(|t| t.name == d.name)
            .map_or_else(
                || format!("tool {}", d.name),
                |t| {
                    format!(
                        "tool {} : {} → {}",
                        t.name, t.input.display, t.output.display
                    )
                },
            ),
        IrDecl::Actor(d) => analysis
            .graph
            .actors
            .iter()
            .find(|a| a.name == d.name)
            .map_or_else(|| format!("actor {}", d.name), actor_header),
        IrDecl::Supervisor(d) => format!(
            "supervisor {} — {}, intensity {}/{}s, children: {}",
            d.name,
            d.strategy,
            d.intensity,
            d.period,
            d.children
                .iter()
                .map(|c| format!("{}: {} ({})", c.id, c.actor, c.restart))
                .collect::<Vec<_>>()
                .join(", "),
        ),
    }
}

/// The one-line header of an actor node: state, mailbox, constructors.
fn actor_header(actor: &ActorNode) -> String {
    let constructors = actor
        .message
        .constructors
        .iter()
        .map(|c| {
            if c.fields.is_empty() {
                c.name.clone()
            } else {
                format!(
                    "{}({})",
                    c.name,
                    c.fields
                        .iter()
                        .map(|f| f.display.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "actor {} — state {}, message {} = {constructors}",
        actor.name, actor.state.display, actor.message.name
    )
}

/// The `effects: {…}` summary line of a declaration, when it has a row.
fn effects_line(analysis: &Analysis, decl: &IrDecl) -> Option<String> {
    let row = match decl {
        IrDecl::Fn(d) => Some(format!("{}", d.effect_row)),
        IrDecl::Extern(d) => fn_row(&d.ty).map(|row| format!("{row}")),
        IrDecl::Tool(d) => {
            // The generated function's full row, including the implicit
            // `Tool<name>` effect.
            let ty = analysis
                .checked
                .bindings
                .get(&tool_fn_name(&d.name))?
                .normalized();
            fn_row(&ty).map(|row| format!("{row}"))
        }
        IrDecl::Actor(d) => Some(format!("{}", d.effect_row)),
        IrDecl::Supervisor(d) => Some(format!("{}", d.effect_row)),
        IrDecl::Type(_) => None,
    }?;
    Some(format!("effects: {row}"))
}

/// The normalized bound type of `name`, or `?` when the checker has none.
fn binding_type(analysis: &Analysis, name: &str) -> String {
    analysis
        .checked
        .bindings
        .get(name)
        .map_or_else(|| String::from("?"), |ty| format!("{}", ty.normalized()))
}

/// The callers and callees of a declaration, as sorted top-level names.
fn call_graph(analysis: &Analysis, decl: &IrDecl) -> (Vec<String>, Vec<String>) {
    let name = decl_name(decl);
    let mut aliases: BTreeSet<String> = BTreeSet::from([String::from(name)]);
    if let IrDecl::Tool(tool) = decl {
        aliases.insert(tool_fn_name(&tool.name));
    }
    let known: BTreeSet<String> = analysis
        .ir
        .declarations
        .iter()
        .flat_map(|d| match d {
            IrDecl::Tool(tool) => vec![tool.name.clone(), tool_fn_name(&tool.name)],
            _ => vec![String::from(decl_name(d))],
        })
        .collect();

    let mut callers: Vec<String> = Vec::new();
    for other in &analysis.ir.declarations {
        if decl_name(other) == name {
            continue;
        }
        if !decl_refs(other).is_disjoint(&aliases) {
            callers.push(String::from(decl_name(other)));
        }
    }
    let callees: Vec<String> = decl_refs(decl)
        .intersection(&known)
        .filter(|r| !aliases.contains(*r))
        .cloned()
        .collect();
    (callers, callees)
}

/// Every top-level name a declaration's bodies reference.
fn decl_refs(decl: &IrDecl) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    match decl {
        IrDecl::Fn(d) => expr_refs(&d.body, &mut out),
        IrDecl::Actor(d) => {
            expr_refs(&d.init.body, &mut out);
            for handler in &d.handlers {
                expr_refs(&handler.body, &mut out);
            }
        }
        IrDecl::Supervisor(d) => {
            for child in &d.children {
                out.insert(child.actor.clone());
                expr_refs(&child.start_args, &mut out);
            }
        }
        IrDecl::Type(_) | IrDecl::Extern(_) | IrDecl::Tool(_) => {}
    }
    out
}

/// Collects the names `expr` references into `out`.
fn expr_refs(expr: &IrExpr, out: &mut BTreeSet<String>) {
    match expr {
        IrExpr::Let(e) => {
            expr_refs(&e.value, out);
            expr_refs(&e.body, out);
        }
        IrExpr::Lambda(e) => expr_refs(&e.body, out),
        IrExpr::App(e) => {
            expr_refs(&e.func, out);
            for arg in &e.args {
                expr_refs(arg, out);
            }
        }
        IrExpr::Match(e) => {
            expr_refs(&e.scrutinee, out);
            for arm in &e.arms {
                expr_refs(&arm.body, out);
            }
        }
        IrExpr::Handle(e) => {
            for arm in &e.arms {
                expr_refs(&arm.handler, out);
            }
            expr_refs(&e.body, out);
        }
        IrExpr::Install(e) => {
            for arm in &e.arms {
                expr_refs(&arm.handler, out);
            }
            expr_refs(&e.body, out);
        }
        IrExpr::Spawn(e) => {
            out.insert(e.actor.clone());
            for arg in &e.args {
                expr_refs(arg, out);
            }
        }
        IrExpr::Supervise(e) => {
            out.insert(e.supervisor.clone());
        }
        IrExpr::Child(e) => {
            out.insert(e.supervisor.clone());
        }
        IrExpr::Send(e) => {
            expr_refs(&e.pid, out);
            expr_refs(&e.message, out);
        }
        IrExpr::Request(e) => {
            expr_refs(&e.pid, out);
            expr_refs(&e.message_fn, out);
        }
        IrExpr::Reply(e) => {
            expr_refs(&e.reply_to, out);
            expr_refs(&e.value, out);
        }
        IrExpr::Crash(e) => expr_refs(&e.message, out),
        IrExpr::Constructor(e) => {
            for arg in &e.args {
                expr_refs(arg, out);
            }
        }
        IrExpr::Literal(_) => {}
        IrExpr::Var(e) => {
            out.insert(e.name.clone());
        }
        IrExpr::Tuple(e) => {
            for elem in &e.elems {
                expr_refs(elem, out);
            }
        }
        IrExpr::List(e) => {
            for elem in &e.elems {
                expr_refs(elem, out);
            }
        }
        IrExpr::Record(e) => {
            for field in &e.fields {
                expr_refs(&field.value, out);
            }
        }
        IrExpr::Field(e) => expr_refs(&e.receiver, out),
    }
}

/// A human-readable explanation of one effect, by head and first argument.
fn explain_effect(head: &str, arg: Option<String>) -> String {
    let arg = arg.unwrap_or_default();
    match head {
        "Tool" => format!(
            "invokes the external tool `{arg}`; each call is checked against the tool's \
             declared signature and recorded on the audit stream"
        ),
        "Send" => format!("sends messages of type `{arg}` to another process (fire-and-forget)"),
        "Await" => format!("blocks awaiting a reply of type `{arg}` to a `request`"),
        "Spawn" => format!("spawns an actor whose mailbox accepts `{arg}`"),
        "Install" => {
            String::from("installs registry-backed default tool handlers for the extent of a block")
        }
        "Supervise" => String::from("starts a declared supervisor's tree"),
        "Exn" => format!("may raise domain errors of type `{arg}`"),
        other => format!("performs the declared effect `{other}`"),
    }
}

/// Approximate token count of `text`, at ~4 characters per token.
fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// Approximate token count of a rendered effect row.
fn estimate_row(row: &EffectRowRef) -> usize {
    estimate_tokens(&row.display)
}

/// `text` truncated to roughly `budget` tokens, ending in an ellipsis.
fn truncate_to(text: &str, budget: usize) -> String {
    let max_chars = budget.saturating_mul(4).saturating_sub(1).max(1);
    let mut out: String = text.chars().take(max_chars).collect();
    if out.chars().count() < text.chars().count() {
        out.push('…');
    }
    out
}

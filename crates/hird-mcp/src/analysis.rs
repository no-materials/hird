// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Lazy per-file compilation and the cache the MCP tools query.
//!
//! One [`Analysis`] is one full run of the compiler pipeline over one source
//! file: parse, type-check as a single-module program, lower to IR, and
//! project the actor/effect graph. [`Cache`] compiles a file on first query
//! and reuses the result until the source text on disk changes. Files that
//! fail to parse or check are not cached; they recompile on the next query.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use cstree::text::TextSize;
use hird_ast::{AstNode, Decl, SourceFile, SyntaxNode, SyntaxToken, TypeExpr};
use hird_check::{CheckedFile, ModuleName};
use hird_ir::{EffectGraph, IrModule};
use hird_parse::SyntaxKind;
use serde_json::json;

use crate::tools::ToolError;

/// The source id every single-file analysis compiles under.
const SOURCE_ID: u32 = 0;

/// One compiled source file and the tables the tools query.
#[derive(Debug)]
pub(crate) struct Analysis {
    /// The file path, as given by the client.
    pub(crate) file: String,
    /// The compiled source text.
    pub(crate) source: String,
    /// The parsed file.
    pub(crate) parsed: SourceFile,
    /// The checker's side tables.
    pub(crate) checked: CheckedFile,
    /// The lowered module.
    pub(crate) ir: IrModule,
    /// The actor/effect graph projection of [`Self::ir`].
    pub(crate) graph: EffectGraph,
    /// Every top-level definition, in source order.
    pub(crate) definitions: Vec<Definition>,
}

/// One name a top-level declaration binds.
#[derive(Debug)]
pub(crate) struct Definition {
    /// The bound name.
    pub(crate) name: String,
    /// The definition kind (`function`, `type`, `constructor`, `effect`,
    /// `tool`, `tool_function`, `actor`, `message_type`,
    /// `message_constructor`, `supervisor`, `extern`).
    pub(crate) kind: &'static str,
    /// 1-based source line of the binding's name token.
    pub(crate) line: usize,
    /// The `//` comment block directly above the declaration, if any.
    pub(crate) doc: Option<String>,
}

impl Analysis {
    /// Compiles `source` as the single module its file stem derives.
    fn compile(file: &str, source: String) -> Result<Self, ToolError> {
        let module = module_name(file)?;
        let parsed = hird_parse::parse(&source, SOURCE_ID);
        let parse_diagnostics: Vec<serde_json::Value> = parsed
            .diagnostics()
            .iter()
            .map(|d| {
                json!({
                    "message": d.message,
                    "line": line_of(&source, d.span.start),
                })
            })
            .collect();
        if !parse_diagnostics.is_empty() {
            return Err(ToolError::with_data(
                "parse_error",
                format!("`{file}` has parse errors"),
                json!({ "diagnostics": parse_diagnostics }),
            ));
        }
        let Some(parsed) = SourceFile::cast(parsed.syntax().clone()) else {
            return Err(ToolError::new(
                "parse_error",
                format!("`{file}` produced no source file"),
            ));
        };

        let name = ModuleName::new(module.clone());
        let program = [(name.clone(), parsed.clone())];
        let Some(checked) = hird_check::check_program(&program).modules.remove(&name) else {
            return Err(ToolError::new(
                "check_error",
                format!("module `{module}` was not checked"),
            ));
        };
        if checked.has_errors() {
            let diagnostics: Vec<serde_json::Value> = checked
                .diagnostics
                .iter()
                .map(|d| {
                    json!({
                        "code": format!("{:?}", d.code),
                        "severity": format!("{:?}", d.severity),
                        "message": d.message,
                        "line": line_of(&source, d.span.start),
                    })
                })
                .collect();
            return Err(ToolError::with_data(
                "check_error",
                format!("`{file}` has type errors"),
                json!({ "diagnostics": diagnostics }),
            ));
        }

        let ir = hird_ir::lower_module(&parsed, &checked, &module);
        let graph = hird_ir::effect_graph(&ir);
        let definitions = index_definitions(&parsed, &source);
        Ok(Self {
            file: String::from(file),
            source,
            parsed,
            checked,
            ir,
            graph,
            definitions,
        })
    }

    /// The most useful token at byte `offset`: an identifier if one touches
    /// it, otherwise the first non-trivia token.
    pub(crate) fn token_at(&self, offset: u32) -> Option<SyntaxToken> {
        let root = self.parsed.syntax();
        if offset > u32::from(root.text_range().end()) {
            return None;
        }
        let candidates: Vec<SyntaxToken> = root
            .token_at_offset(TextSize::from(offset))
            .collect::<Vec<_>>();
        candidates
            .iter()
            .find(|t| t.kind() == SyntaxKind::IDENT)
            .or_else(|| {
                candidates.iter().find(|t| {
                    !matches!(
                        t.kind(),
                        SyntaxKind::WHITESPACE
                            | SyntaxKind::LINE_COMMENT
                            | SyntaxKind::BLOCK_COMMENT
                    )
                })
            })
            .cloned()
    }
}

/// The per-file analysis cache, keyed by the path the client sent.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    /// Successfully compiled files.
    entries: BTreeMap<String, Analysis>,
}

impl Cache {
    /// The analysis of `file`, compiling it on first query or when its
    /// source text changed since the cached compile.
    pub(crate) fn analysis(&mut self, file: &str) -> Result<&Analysis, ToolError> {
        let path = Path::new(file);
        let source = fs::read_to_string(path).map_err(|e| {
            let code = if path.exists() {
                "read_error"
            } else {
                "file_not_found"
            };
            ToolError::new(code, format!("cannot read `{file}`: {e}"))
        })?;
        let stale = self
            .entries
            .get(file)
            .is_none_or(|analysis| analysis.source != source);
        if stale {
            let analysis = Analysis::compile(file, source)?;
            self.entries.insert(String::from(file), analysis);
        }
        Ok(self
            .entries
            .get(file)
            .expect("the entry was just validated or inserted"))
    }
}

/// The module name `file`'s stem derives: each `_`/`-`-separated segment
/// capitalized and concatenated (`agent_planner` → `AgentPlanner`).
fn module_name(file: &str) -> Result<String, ToolError> {
    let stem = Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| ToolError::new("invalid_params", format!("`{file}` has no usable name")))?;
    Ok(stem
        .split(['_', '-'])
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect())
}

/// The 1-based line of byte `offset` in `source`.
pub(crate) fn line_of(source: &str, offset: u32) -> usize {
    let end = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(source.len());
    source[..end].matches('\n').count() + 1
}

/// The byte offset of 1-based `line` and 1-based character `column` in
/// `source`; `None` when the location is outside the text.
pub(crate) fn offset_of(source: &str, line: usize, column: usize) -> Option<u32> {
    if line == 0 || column == 0 {
        return None;
    }
    let start = if line == 1 {
        0
    } else {
        source
            .match_indices('\n')
            .nth(line - 2)
            .map(|(i, _)| i + 1)?
    };
    let text = &source[start..];
    let line_end = text.find('\n').unwrap_or(text.len());
    let byte_in_line = if column == 1 {
        0
    } else {
        text.char_indices().nth(column - 1).map(|(i, _)| i)?
    };
    if byte_in_line > line_end {
        return None;
    }
    u32::try_from(start + byte_in_line).ok()
}

/// The generated function name of a tool: the `PascalCase` tool name in
/// `snake_case`, with acronym runs kept whole (`ReadRepo` → `read_repo`,
/// `LLMCall` → `llm_call`). Mirrors the checker's derivation.
pub(crate) fn tool_fn_name(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = String::with_capacity(bytes.len() + 4);
    for (i, &b) in bytes.iter().enumerate() {
        if b.is_ascii_uppercase() {
            let after_lower = i > 0 && !bytes[i - 1].is_ascii_uppercase();
            let acronym_end = i > 0
                && bytes[i - 1].is_ascii_uppercase()
                && bytes.get(i + 1).is_some_and(u8::is_ascii_lowercase);
            if after_lower || acronym_end {
                out.push('_');
            }
        }
        out.push(b.to_ascii_lowercase() as char);
    }
    out
}

/// Every name `file`'s top-level declarations bind, in source order, with
/// kind, line, and doc comment.
fn index_definitions(file: &SourceFile, source: &str) -> Vec<Definition> {
    let mut out = Vec::new();
    let mut add = |name: &str, kind: &'static str, start: Option<u32>, doc_node: &SyntaxNode| {
        if let Some(start) = start {
            out.push(Definition {
                name: String::from(name),
                kind,
                line: line_of(source, start),
                doc: doc_comment(doc_node),
            });
        }
    };
    for decl in file.declarations() {
        match &decl {
            Decl::Fn(d) => {
                if let Some(name) = d.name() {
                    add(name, "function", name_token_start(d.syntax()), d.syntax());
                }
            }
            Decl::Extern(d) => {
                if let Some(name) = d.name() {
                    add(name, "extern", name_token_start(d.syntax()), d.syntax());
                }
            }
            Decl::Type(d) => {
                if let Some(name) = d.name() {
                    add(name, "type", name_token_start(d.syntax()), d.syntax());
                }
                for ctor in d.constructors() {
                    if let Some(name) = ctor.name() {
                        add(
                            name,
                            "constructor",
                            name_token_start(ctor.syntax()),
                            d.syntax(),
                        );
                    }
                }
            }
            Decl::Effect(d) => {
                if let Some(name) = d.name() {
                    add(name, "effect", name_token_start(d.syntax()), d.syntax());
                }
            }
            Decl::Tool(d) => {
                if let Some(name) = d.name() {
                    let start = name_token_start(d.syntax());
                    add(name, "tool", start, d.syntax());
                    // The declaration also binds its generated function
                    // (`ReadRepo` binds `read_repo`).
                    add(&tool_fn_name(name), "tool_function", start, d.syntax());
                }
            }
            Decl::Actor(d) => {
                if let Some(name) = d.name() {
                    add(name, "actor", name_token_start(d.syntax()), d.syntax());
                }
                for field in d.fields() {
                    if field.name() != Some("message") {
                        continue;
                    }
                    if let Some(TypeExpr::Name(message)) = field.ty() {
                        add(
                            message.text(),
                            "message_type",
                            Some(message.syntax().text_range().start().into()),
                            d.syntax(),
                        );
                    }
                    for ctor in field.constructors() {
                        if let Some(name) = ctor.name() {
                            add(
                                name,
                                "message_constructor",
                                name_token_start(ctor.syntax()),
                                d.syntax(),
                            );
                        }
                    }
                }
            }
            Decl::Supervisor(d) => {
                if let Some(name) = d.name() {
                    add(name, "supervisor", name_token_start(d.syntax()), d.syntax());
                }
            }
            Decl::Module(_) | Decl::Use(_) => {}
        }
    }
    out
}

/// The byte offset of the first `IDENT` token directly under `node` (a
/// declaration's name), or of the node itself when it is name-only.
fn name_token_start(node: &SyntaxNode) -> Option<u32> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text_range().start().into())
        .or_else(|| Some(node.text_range().start().into()))
}

/// The `//` comment block directly above a declaration, with markers
/// stripped and lines joined. The parser attaches leading trivia inside the
/// declaration node, so this scans the node's leading tokens; a blank line
/// discards whatever came before it (a section banner is not a doc).
fn doc_comment(node: &SyntaxNode) -> Option<String> {
    let mut lines: Vec<&str> = Vec::new();
    for element in node.children_with_tokens() {
        let Some(token) = element.as_token() else {
            break;
        };
        match token.kind() {
            SyntaxKind::LINE_COMMENT => {
                let text = token.text().strip_prefix("//").unwrap_or(token.text());
                lines.push(text.strip_prefix(' ').unwrap_or(text));
            }
            SyntaxKind::WHITESPACE if token.text().matches('\n').count() <= 1 => {}
            SyntaxKind::WHITESPACE => lines.clear(),
            _ => break,
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(lines.join("\n"))
}

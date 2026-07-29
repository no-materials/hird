// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Per-file compilation and the queries the server answers from it.
//!
//! One [`Analysis`] is one full run of the compiler front end over one
//! source text: parse, cast, type-check as a single-module program. The
//! server caches it per open document and rebuilds it after every change —
//! no incremental compilation in v0.1.

use std::collections::BTreeMap;

use cstree::text::TextSize;
use hird_ast::{AstNode, Decl, SourceFile, SyntaxNode, SyntaxToken, TypeExpr};
use hird_check::{CheckedFile, ModuleName, NodeKey};
use hird_lex::Span;
use hird_parse::SyntaxKind;
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, GotoDefinitionResponse, Hover,
    HoverContents, Location, MarkupContent, MarkupKind, NumberOrString, Position, Range, Url,
};

use crate::line_index::LineIndex;

/// The source id every single-file analysis compiles under.
const SOURCE_ID: u32 = 0;

/// One compiled source file and the tables the server queries.
#[derive(Debug)]
pub(crate) struct Analysis {
    /// The compiled source text.
    source: String,
    /// Line starts for offset ↔ position conversion.
    line_index: LineIndex,
    /// Parse diagnostics, as span plus message.
    parse_diagnostics: Vec<(Span, &'static str)>,
    /// The parsed file. `None` only if the parser produced no root.
    file: Option<SourceFile>,
    /// The checker's side tables. `None` when parse errors made checking
    /// meaningless.
    checked: Option<CheckedFile>,
    /// Byte range of each top-level definition's name token, keyed by every
    /// name the definition binds.
    definitions: BTreeMap<String, Vec<(u32, u32)>>,
}

impl Analysis {
    /// Compiles `source` as the single module `module`.
    pub(crate) fn new(module: &str, source: String) -> Self {
        let parsed = hird_parse::parse(&source, SOURCE_ID);
        let parse_diagnostics: Vec<(Span, &'static str)> = parsed
            .diagnostics()
            .iter()
            .map(|d| (d.span, d.message))
            .collect();
        let file = SourceFile::cast(parsed.syntax().clone());
        let checked = match (&file, parse_diagnostics.is_empty()) {
            (Some(file), true) => {
                let name = ModuleName::new(module);
                let program = [(name.clone(), file.clone())];
                hird_check::check_program(&program).modules.remove(&name)
            }
            _ => None,
        };
        let definitions = file.as_ref().map(index_definitions).unwrap_or_default();
        let line_index = LineIndex::new(&source);
        Self {
            source,
            line_index,
            parse_diagnostics,
            file,
            checked,
            definitions,
        }
    }

    /// All diagnostics as LSP values: parse errors when there are any,
    /// otherwise the checker's errors and warnings.
    pub(crate) fn diagnostics(&self, uri: &Url) -> Vec<Diagnostic> {
        let mut out: Vec<Diagnostic> = self
            .parse_diagnostics
            .iter()
            .map(|(span, message)| Diagnostic {
                range: self.range(*span),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some(String::from("hird")),
                message: String::from(*message),
                ..Diagnostic::default()
            })
            .collect();
        if let Some(checked) = &self.checked {
            out.extend(checked.diagnostics.iter().map(|d| {
                let related: Vec<DiagnosticRelatedInformation> = d
                    .related
                    .iter()
                    .map(|r| DiagnosticRelatedInformation {
                        location: Location {
                            uri: uri.clone(),
                            range: self.range(r.span),
                        },
                        message: r.message.clone(),
                    })
                    .collect();
                Diagnostic {
                    range: self.range(d.span),
                    severity: Some(match d.severity {
                        hird_check::Severity::Error => DiagnosticSeverity::ERROR,
                        hird_check::Severity::Warning => DiagnosticSeverity::WARNING,
                    }),
                    code: Some(NumberOrString::String(format!("{:?}", d.code))),
                    source: Some(String::from("hird")),
                    message: d.message.clone(),
                    related_information: (!related.is_empty()).then_some(related),
                    ..Diagnostic::default()
                }
            }));
        }
        out
    }

    /// The inferred type of the identifier (or enclosing expression) at
    /// `position`, as `name : Type` hover markdown.
    pub(crate) fn hover(&self, position: Position) -> Option<Hover> {
        let checked = self.checked.as_ref()?;
        let token = self.token_at(position)?;
        let ty = checked
            .type_at(NodeKey::of_token(&token))
            .or_else(|| {
                token
                    .ancestors()
                    .find_map(|node| checked.type_at(NodeKey::of_node(node)))
            })
            .or_else(|| {
                (token.kind() == SyntaxKind::IDENT)
                    .then(|| checked.bindings.get(token.text()))
                    .flatten()
            })?;
        let rendered = ty.normalized().to_string();
        let value = if token.kind() == SyntaxKind::IDENT {
            format!("{} : {}", token.text(), rendered)
        } else {
            rendered
        };
        let range = token.text_range();
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("```hird\n{value}\n```"),
            }),
            range: Some(self.range(Span::new(
                range.start().into(),
                range.end().into(),
                SOURCE_ID,
            ))),
        })
    }

    /// The top-level definition sites of the identifier at `position`.
    pub(crate) fn definition(
        &self,
        uri: &Url,
        position: Position,
    ) -> Option<GotoDefinitionResponse> {
        let token = self.token_at(position)?;
        if token.kind() != SyntaxKind::IDENT {
            return None;
        }
        let mut locations: Vec<Location> = self
            .definitions
            .get(token.text())?
            .iter()
            .map(|&(start, end)| Location {
                uri: uri.clone(),
                range: self.range(Span::new(start, end, SOURCE_ID)),
            })
            .collect();
        match locations.len() {
            0 => None,
            1 => Some(GotoDefinitionResponse::Scalar(locations.remove(0))),
            _ => Some(GotoDefinitionResponse::Array(locations)),
        }
    }

    /// The most useful token at `position`: an identifier if one touches it,
    /// otherwise the first non-trivia token.
    fn token_at(&self, position: Position) -> Option<SyntaxToken> {
        let file = self.file.as_ref()?;
        let offset = self.line_index.offset(&self.source, position)?;
        let root = file.syntax();
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

    /// `span` as an LSP range.
    fn range(&self, span: Span) -> Range {
        Range {
            start: self.line_index.position(&self.source, span.start),
            end: self.line_index.position(&self.source, span.end),
        }
    }
}

/// Every name a file's top-level declarations bind, mapped to the byte range
/// of the binding's name token: functions, externs, types and their
/// constructors, effects, tools (marker and generated function), actors and
/// their message types and constructors, and supervisors.
fn index_definitions(file: &SourceFile) -> BTreeMap<String, Vec<(u32, u32)>> {
    let mut out: BTreeMap<String, Vec<(u32, u32)>> = BTreeMap::new();
    let mut add = |name: &str, range: Option<(u32, u32)>| {
        if let Some(range) = range {
            out.entry(String::from(name)).or_default().push(range);
        }
    };
    for decl in file.declarations() {
        match &decl {
            Decl::Fn(d) => {
                if let Some(name) = d.name() {
                    add(name, name_token_range(d.syntax()));
                }
            }
            Decl::Extern(d) => {
                if let Some(name) = d.name() {
                    add(name, name_token_range(d.syntax()));
                }
            }
            Decl::Type(d) => {
                if let Some(name) = d.name() {
                    add(name, name_token_range(d.syntax()));
                }
                for ctor in d.constructors() {
                    if let Some(name) = ctor.name() {
                        add(name, name_token_range(ctor.syntax()));
                    }
                }
            }
            Decl::Effect(d) => {
                if let Some(name) = d.name() {
                    add(name, name_token_range(d.syntax()));
                }
            }
            Decl::Tool(d) => {
                if let Some(name) = d.name() {
                    let range = name_token_range(d.syntax());
                    add(name, range);
                    // The declaration also binds its generated function
                    // (`ReadRepo` binds `read_repo`).
                    add(&tool_fn_name(name), range);
                }
            }
            Decl::Actor(d) => {
                if let Some(name) = d.name() {
                    add(name, name_token_range(d.syntax()));
                }
                for field in d.fields() {
                    if field.name() != Some("message") {
                        continue;
                    }
                    if let Some(TypeExpr::Name(message)) = field.ty() {
                        add(message.text(), Some(token_range(message.syntax())));
                    }
                    for ctor in field.constructors() {
                        if let Some(name) = ctor.name() {
                            add(name, name_token_range(ctor.syntax()));
                        }
                    }
                }
            }
            Decl::Supervisor(d) => {
                if let Some(name) = d.name() {
                    add(name, name_token_range(d.syntax()));
                }
            }
            Decl::Module(_) | Decl::Use(_) => {}
        }
    }
    out
}

/// The byte range of the first `IDENT` token directly under `node` (a
/// declaration's name).
fn name_token_range(node: &SyntaxNode) -> Option<(u32, u32)> {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(token_range)
}

/// The byte range of `token`.
fn token_range(token: &SyntaxToken) -> (u32, u32) {
    let range = token.text_range();
    (range.start().into(), range.end().into())
}

/// The generated function name of a tool: the `PascalCase` tool name in
/// `snake_case`, with acronym runs kept whole (`ReadRepo` → `read_repo`,
/// `LLMCall` → `llm_call`). Mirrors the checker's derivation.
fn tool_fn_name(name: &str) -> String {
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

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Whole-program compilation and the queries the server answers from it.
//!
//! One [`Program`] is one full run of the compiler front end over a
//! directory of modules: every member parses, and the members that parsed
//! type-check together, so `use` imports resolve to their sibling modules.
//! The server caches one program per directory and rebuilds it after every
//! change to a document in that directory — no incremental compilation in
//! v0.1.

use std::collections::BTreeMap;

use cstree::text::TextSize;
use hird_ast::{AstNode, Decl, Expr, FieldExpr, SourceFile, SyntaxNode, SyntaxToken, TypeExpr};
use hird_check::{CheckedFile, ModuleName, NodeKey};
use hird_lex::Span;
use hird_parse::SyntaxKind;
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, GotoDefinitionResponse, Hover,
    HoverContents, Location, MarkupContent, MarkupKind, NumberOrString, Position, Range, Url,
};

use crate::line_index::LineIndex;

/// One directory of modules, compiled as a whole.
#[derive(Debug)]
pub(crate) struct Program {
    /// The members, in URI order.
    modules: Vec<Module>,
    /// The module index behind each checker source id (only members that
    /// parsed are checked, so ids and indices differ).
    checked_ids: Vec<usize>,
}

/// One compiled source file and the tables the server queries.
#[derive(Debug)]
struct Module {
    /// The document URI.
    uri: Url,
    /// The compiled source text.
    source: String,
    /// Line starts for offset ↔ position conversion.
    line_index: LineIndex,
    /// Parse diagnostics, as span plus message.
    parse_diagnostics: Vec<(Span, &'static str)>,
    /// The parsed file. `None` only if the parser produced no root.
    file: Option<SourceFile>,
    /// The checker's side tables. `None` when parse errors kept the module
    /// out of the checked program.
    checked: Option<CheckedFile>,
    /// Byte range of each top-level definition's name token, keyed by every
    /// name the definition binds.
    definitions: BTreeMap<String, Vec<(u32, u32)>>,
    /// The module's `use` imports, in source order.
    imports: Vec<Import>,
}

/// One `use` declaration, resolved against the program.
#[derive(Debug)]
struct Import {
    /// Index of the imported module; `None` when no member has that name.
    target: Option<usize>,
    /// The qualifier a whole-module or aliased import binds (`use Util` ⇒
    /// `Util`, `use Util as U` ⇒ `U`); `None` for a selective import.
    qualifier: Option<String>,
    /// The members a selective import binds unqualified.
    selected: Vec<String>,
}

impl Program {
    /// Compiles `members` (document URI, source text) as one program; each
    /// module is named after its URI's file stem.
    pub(crate) fn new(members: Vec<(Url, String)>) -> Self {
        let mut modules: Vec<Module> = Vec::with_capacity(members.len());
        let mut checked_ids: Vec<usize> = Vec::new();
        let mut to_check: Vec<(ModuleName, SourceFile)> = Vec::new();
        for (uri, source) in members {
            let parsed = hird_parse::parse(&source, id_of(to_check.len()));
            let parse_diagnostics: Vec<(Span, &'static str)> = parsed
                .diagnostics()
                .iter()
                .map(|d| (d.span, d.message))
                .collect();
            let file = SourceFile::cast(parsed.syntax().clone());
            if let (Some(file), true) = (&file, parse_diagnostics.is_empty()) {
                checked_ids.push(modules.len());
                to_check.push((ModuleName::new(module_name_of(&uri)), file.clone()));
            }
            let definitions = file.as_ref().map(index_definitions).unwrap_or_default();
            let line_index = LineIndex::new(&source);
            modules.push(Module {
                uri,
                source,
                line_index,
                parse_diagnostics,
                file,
                checked: None,
                definitions,
                imports: Vec::new(),
            });
        }

        let index: BTreeMap<String, usize> = to_check
            .iter()
            .zip(&checked_ids)
            .map(|((name, _), &module)| (String::from(name.as_str()), module))
            .collect();
        let mut checked = hird_check::check_program(&to_check);
        for ((name, file), &i) in to_check.iter().zip(&checked_ids) {
            modules[i].checked = checked.modules.remove(name);
            modules[i].imports = index_imports(file, &index);
        }
        Self {
            modules,
            checked_ids,
        }
    }

    /// All diagnostics of the document `uri` as LSP values: parse errors
    /// when there are any, otherwise the checker's errors and warnings.
    /// Empty for a URI outside the program.
    pub(crate) fn diagnostics(&self, uri: &Url) -> Vec<Diagnostic> {
        let Some(module) = self.module(uri) else {
            return Vec::new();
        };
        let mut out: Vec<Diagnostic> = module
            .parse_diagnostics
            .iter()
            .map(|(span, message)| Diagnostic {
                range: module.range(*span),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some(String::from("hird")),
                message: String::from(*message),
                ..Diagnostic::default()
            })
            .collect();
        if let Some(checked) = &module.checked {
            out.extend(checked.diagnostics.iter().map(|d| {
                let related: Vec<DiagnosticRelatedInformation> = d
                    .related
                    .iter()
                    .filter_map(|r| {
                        Some(DiagnosticRelatedInformation {
                            location: self.location(r.span)?,
                            message: r.message.clone(),
                        })
                    })
                    .collect();
                Diagnostic {
                    range: module.range(d.span),
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
    /// `position` in `uri`, as `name : Type` hover markdown.
    pub(crate) fn hover(&self, uri: &Url, position: Position) -> Option<Hover> {
        let module = self.module(uri)?;
        let checked = module.checked.as_ref()?;
        let token = module.token_at(position)?;
        let ty = checked
            .type_at(NodeKey::of_token(&token))
            .or_else(|| {
                token
                    .ancestors()
                    .find_map(|node| checked.type_at(NodeKey::of_node(node)))
            })
            .or_else(|| {
                // A name outside any expression (a declaration or `use`
                // member): its binding, wherever it is defined.
                (token.kind() == SyntaxKind::IDENT)
                    .then(|| self.defining_module(module, &token))
                    .flatten()
                    .and_then(|defining| defining.checked.as_ref()?.bindings.get(token.text()))
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
            range: Some(module.range(Span::new(range.start().into(), range.end().into(), 0))),
        })
    }

    /// The top-level definition sites of the identifier at `position` in
    /// `uri`, in whichever module defines it.
    pub(crate) fn definition(
        &self,
        uri: &Url,
        position: Position,
    ) -> Option<GotoDefinitionResponse> {
        let module = self.module(uri)?;
        let token = module.token_at(position)?;
        if token.kind() != SyntaxKind::IDENT {
            return None;
        }
        let defining = self.defining_module(module, &token)?;
        let mut locations: Vec<Location> = defining
            .definitions
            .get(token.text())?
            .iter()
            .map(|&(start, end)| Location {
                uri: defining.uri.clone(),
                range: defining.range(Span::new(start, end, 0)),
            })
            .collect();
        match locations.len() {
            0 => None,
            1 => Some(GotoDefinitionResponse::Scalar(locations.remove(0))),
            _ => Some(GotoDefinitionResponse::Array(locations)),
        }
    }

    /// The module whose top-level declarations bind the identifier `token`
    /// of `module`: `module` itself, the target of a whole-module import
    /// when the token is the member of a `Qualifier.member` access, or the
    /// target of a selective import naming it.
    fn defining_module<'a>(
        &'a self,
        module: &'a Module,
        token: &SyntaxToken,
    ) -> Option<&'a Module> {
        let name = token.text();
        if module.definitions.contains_key(name) {
            return Some(module);
        }
        let import = match qualifier_of(token) {
            Some(qualifier) => module
                .imports
                .iter()
                .find(|i| i.qualifier.as_deref() == Some(qualifier.text())),
            None => module
                .imports
                .iter()
                .find(|i| i.selected.iter().any(|s| s == name)),
        }?;
        let target = self.modules.get(import.target?)?;
        target.definitions.contains_key(name).then_some(target)
    }

    /// The module of the document `uri`.
    fn module(&self, uri: &Url) -> Option<&Module> {
        self.modules.iter().find(|m| &m.uri == uri)
    }

    /// The LSP location of a checker span, resolved through its source id.
    fn location(&self, span: Span) -> Option<Location> {
        let id = usize::try_from(span.source_id).ok()?;
        let module = &self.modules[*self.checked_ids.get(id)?];
        Some(Location {
            uri: module.uri.clone(),
            range: module.range(span),
        })
    }
}

impl Module {
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

    /// `span`'s byte offsets as an LSP range in this module's text.
    fn range(&self, span: Span) -> Range {
        Range {
            start: self.line_index.position(&self.source, span.start),
            end: self.line_index.position(&self.source, span.end),
        }
    }
}

/// The `u32` source id of checked-slice index `i` (the `check_program`
/// convention).
fn id_of(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// The module name a document URI derives, from its file stem: each
/// `_`/`-`-separated segment capitalized and concatenated
/// (`repo_utils.hird` → `RepoUtils`), matching the CLI's derivation.
pub(crate) fn module_name_of(uri: &Url) -> String {
    let path = uri.path();
    let stem = path.rsplit('/').next().unwrap_or(path);
    let stem = stem.strip_suffix(".hird").unwrap_or(stem);
    stem.split(['_', '-'])
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

/// The qualifier token when `token` is the member of a `Qualifier.member`
/// access whose receiver is a bare name (`Util.show` ⇒ `Util`).
fn qualifier_of(token: &SyntaxToken) -> Option<SyntaxToken> {
    let field = FieldExpr::cast(token.parent().clone())?;
    let Expr::Name(receiver) = field.receiver()? else {
        return None;
    };
    (receiver.syntax().text_range() != token.text_range()).then(|| receiver.syntax().clone())
}

/// `file`'s `use` declarations resolved against the program `index` (module
/// name → module index).
fn index_imports(file: &SourceFile, index: &BTreeMap<String, usize>) -> Vec<Import> {
    file.declarations()
        .filter_map(|decl| match decl {
            Decl::Use(u) => Some(u),
            _ => None,
        })
        .filter_map(|u| {
            let path = u.path()?;
            let target_name = path.segments().collect::<Vec<_>>().join(".");
            let selected: Vec<String> = u.selected().map(String::from).collect();
            let qualifier = selected.is_empty().then(|| {
                u.alias()
                    .or_else(|| path.segments().last())
                    .map(String::from)
                    .unwrap_or_default()
            });
            Some(Import {
                target: index.get(target_name.as_str()).copied(),
                qualifier,
                selected,
            })
        })
        .collect()
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
            Decl::TypeAlias(d) => {
                if let Some(name) = d.name() {
                    add(name, name_token_range(d.syntax()));
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

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Lazy program compilation and the cache the MCP tools query.
//!
//! The unit of compilation is a directory: every `.hird` file in the queried
//! file's directory is a module of one [`Program`], named after its file stem
//! exactly as the CLI names it. The program is parsed, type-checked as a
//! whole (so `use` imports resolve to their sibling modules), and each
//! error-free module is lowered to IR and projected to its actor/effect
//! graph. [`Cache`] compiles a directory on first query and reuses the
//! result until any member's source text changes on disk.
//!
//! A sibling that fails to parse contributes nothing and is left out of the
//! program; a sibling with type errors stays checked (its exports still
//! resolve) but has no IR. Only the queried file's own parse or type errors
//! fail a query.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cstree::text::TextSize;
use hird_ast::{AstNode, Decl, SourceFile, SyntaxNode, SyntaxToken, TypeExpr};
use hird_check::{CheckedFile, ModuleName};
use hird_ir::{EffectGraph, IrModule};
use hird_parse::SyntaxKind;
use serde_json::{Value, json};

use crate::tools::ToolError;

/// One directory of modules, compiled as a whole.
#[derive(Debug)]
pub(crate) struct Program {
    /// The modules that parsed, in file-name order; a module's index is the
    /// source id its diagnostics carry.
    pub(crate) modules: Vec<Module>,
    /// The members that did not parse: file plus parse diagnostics.
    skipped: Vec<(String, Value)>,
    /// Every member's `(file, source text)`, for staleness checks.
    sources: Vec<(String, String)>,
}

/// One compiled module and the tables the tools query.
#[derive(Debug)]
pub(crate) struct Module {
    /// The file path, in the queried path's directory.
    pub(crate) file: String,
    /// The path-derived module name.
    pub(crate) name: String,
    /// The compiled source text.
    pub(crate) source: String,
    /// The parsed file.
    pub(crate) parsed: SourceFile,
    /// The checker's side tables.
    pub(crate) checked: CheckedFile,
    /// The lowered module; `None` when the module has type errors.
    ir: Option<IrModule>,
    /// The actor/effect graph projection of the IR; `None` with it.
    graph: Option<EffectGraph>,
    /// Every top-level definition, in source order.
    pub(crate) definitions: Vec<Definition>,
    /// The module's `use` imports, in source order.
    imports: Vec<Import>,
}

/// One `use` declaration, resolved against the program.
#[derive(Debug)]
struct Import {
    /// Index of the imported module in [`Program::modules`]; `None` when no
    /// member has that name.
    target: Option<usize>,
    /// The qualifier a whole-module or aliased import binds (`use Util` ⇒
    /// `Util`, `use Util as U` ⇒ `U`); `None` for a selective import.
    qualifier: Option<String>,
    /// The members a selective import binds unqualified.
    selected: Vec<String>,
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

impl Program {
    /// Compiles `sources` (file path, text) as one program.
    fn compile(sources: Vec<(String, String)>) -> Result<Self, ToolError> {
        let mut skipped = Vec::new();
        let mut parsed: Vec<(String, String, String, SourceFile)> = Vec::new();
        for (file, source) in &sources {
            let name = module_name(file)?;
            let tree = hird_parse::parse(source, id_of(parsed.len()));
            let diagnostics: Vec<Value> = tree
                .diagnostics()
                .iter()
                .map(|d| {
                    json!({
                        "message": d.message,
                        "line": line_of(source, d.span.start),
                    })
                })
                .collect();
            let root = SourceFile::cast(tree.syntax().clone());
            match root {
                Some(root) if diagnostics.is_empty() => {
                    parsed.push((file.clone(), name, source.clone(), root));
                }
                Some(_) => skipped.push((file.clone(), json!({ "diagnostics": diagnostics }))),
                None => skipped.push((
                    file.clone(),
                    json!({ "diagnostics": [{ "message": "no source file produced", "line": 1 }] }),
                )),
            }
        }

        let index: BTreeMap<String, usize> = parsed
            .iter()
            .enumerate()
            .map(|(i, (_, name, _, _))| (name.clone(), i))
            .collect();
        let program: Vec<(ModuleName, SourceFile)> = parsed
            .iter()
            .map(|(_, name, _, root)| (ModuleName::new(name.clone()), root.clone()))
            .collect();
        let mut checked_program = hird_check::check_program(&program);

        let mut modules = Vec::with_capacity(parsed.len());
        for (file, name, source, root) in parsed {
            let imports = index_imports(&root, &index);
            let Some(checked) = checked_program
                .modules
                .remove(&ModuleName::new(name.clone()))
            else {
                return Err(ToolError::new(
                    "check_error",
                    format!("module `{name}` was not checked"),
                ));
            };
            let ir = (!checked.has_errors()).then(|| hird_ir::lower_module(&root, &checked, &name));
            let graph = ir.as_ref().map(hird_ir::effect_graph);
            let definitions = index_definitions(&root, &source);
            modules.push(Module {
                file,
                name,
                source,
                parsed: root,
                checked,
                ir,
                graph,
                definitions,
                imports,
            });
        }
        Ok(Self {
            modules,
            skipped,
            sources,
        })
    }

    /// The queried module `file` (matched by file name), or its own parse or
    /// type errors as a tool error.
    fn query(&self, file: &str) -> Result<Query<'_>, ToolError> {
        let wanted = Path::new(file).file_name();
        if let Some((_, data)) = self
            .skipped
            .iter()
            .find(|(f, _)| Path::new(f).file_name() == wanted)
        {
            return Err(ToolError::with_data(
                "parse_error",
                format!("`{file}` has parse errors"),
                data.clone(),
            ));
        }
        let module = self
            .modules
            .iter()
            .find(|m| Path::new(&m.file).file_name() == wanted)
            .ok_or_else(|| {
                ToolError::new("invalid_params", format!("`{file}` is not a .hird file"))
            })?;
        if module.checked.has_errors() {
            let mut data = json!({ "diagnostics": diagnostics_json(module) });
            if !self.skipped.is_empty() {
                data["siblings_with_parse_errors"] = self
                    .skipped
                    .iter()
                    .map(|(f, d)| json!({ "file": f, "diagnostics": d["diagnostics"] }))
                    .collect();
            }
            return Err(ToolError::with_data(
                "check_error",
                format!("`{file}` has type errors"),
                data,
            ));
        }
        Ok(Query {
            program: self,
            module,
        })
    }
}

impl Module {
    /// The lowered module, or a `check_error` when type errors prevented
    /// lowering.
    pub(crate) fn ir(&self) -> Result<&IrModule, ToolError> {
        self.ir.as_ref().ok_or_else(|| self.check_error())
    }

    /// The actor/effect graph, or a `check_error` when type errors prevented
    /// lowering.
    pub(crate) fn graph(&self) -> Result<&EffectGraph, ToolError> {
        self.graph.as_ref().ok_or_else(|| self.check_error())
    }

    /// The `check_error` for this module's diagnostics.
    fn check_error(&self) -> ToolError {
        ToolError::with_data(
            "check_error",
            format!("`{}` has type errors", self.file),
            json!({ "diagnostics": diagnostics_json(self) }),
        )
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

    /// Whether a top-level declaration binds `name`.
    fn defines(&self, name: &str) -> bool {
        self.definitions.iter().any(|d| d.name == name)
    }

    /// The names of this module's selective imports (bound unqualified).
    fn selected_imports(&self) -> impl Iterator<Item = &str> {
        self.imports
            .iter()
            .flat_map(|i| i.selected.iter().map(String::as_str))
    }
}

/// One tool query: the program and the module the client asked about.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Query<'a> {
    /// The whole program.
    pub(crate) program: &'a Program,
    /// The queried module.
    pub(crate) module: &'a Module,
}

impl<'a> Query<'a> {
    /// Resolves `name` the way the queried module's source would: a local
    /// definition, a selectively imported member, or `Qualifier.member`
    /// through a whole-module import. Returns the defining module and the
    /// bare member name.
    pub(crate) fn resolve(&self, name: &str) -> Option<(&'a Module, &'a str)> {
        let (module, member) = match name.rsplit_once('.') {
            Some((qualifier, member)) => (
                self.imported_via(|i| i.qualifier.as_deref() == Some(qualifier))?,
                member,
            ),
            None if self.module.defines(name) => (self.module, name),
            None => (
                self.imported_via(|i| i.selected.iter().any(|s| s == name))?,
                name,
            ),
        };
        module
            .definitions
            .iter()
            .find(|d| d.name == member)
            .map(|d| (module, d.name.as_str()))
    }

    /// The module behind the first of the queried module's imports matching
    /// `pred`, if that import resolved.
    fn imported_via(&self, pred: impl Fn(&Import) -> bool) -> Option<&'a Module> {
        self.module
            .imports
            .iter()
            .find(|i| pred(i))
            .and_then(|i| self.program.modules.get(i.target?))
    }

    /// Every name the queried module's source can refer to unqualified: its
    /// own definitions and its selective imports.
    pub(crate) fn names_in_scope(&self) -> impl Iterator<Item = &'a str> {
        self.module
            .definitions
            .iter()
            .map(|d| d.name.as_str())
            .chain(self.module.selected_imports())
    }

    /// The names by which `module` refers to `target`'s definition `name`:
    /// the name itself within `target`, the bare name where selectively
    /// imported, and `Qualifier.name` for each whole-module import.
    pub(crate) fn names_for(&self, module: &Module, target: &Module, name: &str) -> Vec<String> {
        if std::ptr::eq(module, target) {
            return vec![String::from(name)];
        }
        let target_index = self
            .program
            .modules
            .iter()
            .position(|m| std::ptr::eq(m, target));
        module
            .imports
            .iter()
            .filter(|i| i.target == target_index)
            .filter_map(|i| match &i.qualifier {
                Some(q) => Some(format!("{q}.{name}")),
                None => i
                    .selected
                    .iter()
                    .any(|s| s == name)
                    .then(|| String::from(name)),
            })
            .collect()
    }
}

/// The program cache, one entry per directory.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    /// Compiled directories, keyed by directory path.
    entries: BTreeMap<PathBuf, Program>,
}

impl Cache {
    /// The query for `file`: its directory compiled as a program (on first
    /// query, or when any member's source text changed) and the module
    /// `file` names.
    pub(crate) fn query(&mut self, file: &str) -> Result<Query<'_>, ToolError> {
        let path = Path::new(file);
        // Read the queried file first so a missing or unreadable path is
        // reported as such, before its directory is scanned.
        fs::read_to_string(path).map_err(|e| {
            let code = if path.exists() {
                "read_error"
            } else {
                "file_not_found"
            };
            ToolError::new(code, format!("cannot read `{file}`: {e}"))
        })?;
        let dir = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let sources = read_directory(&dir)?;
        let stale = self
            .entries
            .get(&dir)
            .is_none_or(|program| program.sources != sources);
        if stale {
            let program = Program::compile(sources)?;
            self.entries.insert(dir.clone(), program);
        }
        self.entries
            .get(&dir)
            .expect("the entry was just validated or inserted")
            .query(file)
    }
}

/// Every readable `.hird` file in `dir` with its text, in file-name order.
fn read_directory(dir: &Path) -> Result<Vec<(String, String)>, ToolError> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| {
            ToolError::new(
                "read_error",
                format!("cannot read directory `{}`: {e}", dir.display()),
            )
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "hird"))
        .collect();
    paths.sort();
    Ok(paths
        .into_iter()
        .filter_map(|path| {
            let source = fs::read_to_string(&path).ok()?;
            Some((path.to_string_lossy().into_owned(), source))
        })
        .collect())
}

/// The `u32` source id of program index `i` (the `check_program` convention).
fn id_of(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// A module's checker diagnostics as JSON.
fn diagnostics_json(module: &Module) -> Vec<Value> {
    module
        .checked
        .diagnostics
        .iter()
        .map(|d| {
            json!({
                "code": format!("{:?}", d.code),
                "severity": format!("{:?}", d.severity),
                "message": d.message,
                "line": line_of(&module.source, d.span.start),
            })
        })
        .collect()
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
            Decl::TypeAlias(d) => {
                if let Some(name) = d.name() {
                    add(name, "type alias", name_token_start(d.syntax()), d.syntax());
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
/// declaration node (inside its `VISIBILITY` child for a `pub` declaration),
/// so this scans that node's leading tokens; a blank line discards whatever
/// came before it (a section banner is not a doc).
fn doc_comment(node: &SyntaxNode) -> Option<String> {
    let first_child = node.children_with_tokens().next();
    let owner: SyntaxNode = match first_child.and_then(|e| e.into_node().cloned()) {
        Some(visibility) if visibility.kind() == SyntaxKind::VISIBILITY => visibility,
        _ => node.clone(),
    };
    let mut lines: Vec<&str> = Vec::new();
    for element in owner.children_with_tokens() {
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

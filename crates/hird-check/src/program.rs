// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Whole-program checking: the module graph, import resolution, and
//! cross-module visibility.
//!
//! [`check_program`] is the multi-module entry point. It validates each file's
//! `module` declaration against its path-derived name, builds the import graph,
//! rejects cycles, then checks modules in dependency order — seeding every
//! module's environment from the *exported* surface of the modules it imports.
//! The single-file [`crate::check`] stays the per-module core; this driver
//! wraps it with seeding and ordering.
//!
//! Standard-library resolution is deferred: a `use` that names no in-program
//! module is reported as unresolved rather than searched for on a path.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hird_ast::{AstNode, Decl, SourceFile, UseDecl};
use hird_lex::Span;
use hird_types::{Name, Type};

use crate::checker::{AliasExpansion, Checker, tarjan};
use crate::diag::{CheckCode, CheckDiagnostic};
use crate::{CheckedFile, ModuleName, node_span, token_span};

/// The public surface one module presents to the modules that import it.
///
/// Holds exported (`pub`) functions, types, and aliases. Each exported type carries its
/// constructor schemes — usable for a transparent type, diagnostic-only for an
/// opaque one (so an out-of-module destructure names the type rather than
/// reporting an unknown constructor).
#[derive(Debug)]
pub(crate) struct ModuleInterface {
    /// Exported function name → generalised scheme.
    pub(crate) functions: BTreeMap<String, Type>,
    /// Exported type name → its export record.
    pub(crate) types: BTreeMap<Name, ExportedType>,
    /// Exported alias name → its expansion; an importer sees the expansion.
    pub(crate) aliases: BTreeMap<Name, AliasExpansion>,
}

/// An exported type's importable shape.
#[derive(Debug)]
pub(crate) struct ExportedType {
    /// Type-parameter count.
    pub(crate) arity: usize,
    /// Whether the type is opaque (constructors stay module-private).
    pub(crate) opaque: bool,
    /// Constructor name → scheme, in declaration order.
    pub(crate) ctors: Vec<(Name, Type)>,
}

impl ModuleInterface {
    /// Every importable constructor — those of transparent exported types —
    /// as `(owning type, constructor, scheme)`. Opaque types' constructors
    /// stay module-private and are skipped.
    fn transparent_ctors(&self) -> impl Iterator<Item = (&Name, &Name, &Type)> {
        self.types
            .iter()
            .filter(|(_, ty)| !ty.opaque)
            .flat_map(|(owner, ty)| {
                ty.ctors
                    .iter()
                    .map(move |(ctor, scheme)| (owner, ctor, scheme))
            })
    }

    /// The scheme of an importable constructor, with its owning type.
    fn public_ctor(&self, name: &str) -> Option<(Name, Type)> {
        self.transparent_ctors()
            .find(|(_, ctor, _)| ctor.as_str() == name)
            .map(|(owner, _, scheme)| (owner.clone(), scheme.clone()))
    }

    /// Every value reachable through a qualifier (`Mod.member`): exported
    /// functions plus the constructors of transparent exported types.
    fn exported_values(&self) -> BTreeMap<String, Type> {
        let mut values = self.functions.clone();
        for (_, ctor, scheme) in self.transparent_ctors() {
            values.insert(String::from(ctor.as_str()), scheme.clone());
        }
        values
    }
}

/// The result of checking a whole program: every module's checked file, keyed
/// by module name.
#[derive(Debug)]
pub struct CheckedProgram {
    /// Per-module results, in module-name order.
    pub modules: BTreeMap<ModuleName, CheckedFile>,
}

impl CheckedProgram {
    /// Whether any module reported an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.modules.values().any(CheckedFile::has_errors)
    }
}

/// One resolved `use` import within a module.
struct ResolvedUse {
    /// The imported module's name (the dotted path as written).
    target: ModuleName,
    /// Index of `target` in the program, or `None` when unresolved.
    target_index: Option<usize>,
    /// Span of the whole `use` declaration (cycle diagnostics).
    use_span: Span,
    /// Span of the path (unresolved-module diagnostics).
    path_span: Span,
    /// Explicit `as` alias, if any.
    alias: Option<String>,
    /// Selective members and their name spans; empty for whole-module and
    /// aliased forms.
    selected: Vec<(String, Span)>,
}

impl ResolvedUse {
    /// The qualifier a whole-module or aliased import binds: the alias if
    /// given, otherwise the target's trailing segment.
    fn qualifier(&self) -> &str {
        self.alias
            .as_deref()
            .unwrap_or_else(|| self.target.last_segment())
    }
}

/// Type-checks a whole program: a slice of `(module name, parsed file)` pairs.
///
/// Module names are authoritative (path-derived by the caller) and validated
/// against each file's `module` declaration. Imports resolve only to other
/// modules in the slice; cycles are rejected. Each module is checked with its
/// imports' exported schemes in scope.
#[must_use]
pub fn check_program(modules: &[(ModuleName, SourceFile)]) -> CheckedProgram {
    let index: BTreeMap<&str, usize> = modules
        .iter()
        .enumerate()
        .map(|(i, (name, _))| (name.as_str(), i))
        .collect();

    // Resolve every module's imports and build the dependency graph.
    let mut resolved: Vec<Vec<ResolvedUse>> = Vec::new();
    let mut graph: Vec<Vec<usize>> = Vec::new();
    for (i, (_, file)) in modules.iter().enumerate() {
        let uses = resolve_uses(file, &index, i);
        let mut edges: Vec<usize> = uses.iter().filter_map(|u| u.target_index).collect();
        edges.sort_unstable();
        edges.dedup();
        graph.push(edges);
        resolved.push(uses);
    }

    // Strongly connected components, callees first. An import whose target
    // shares the importer's component is a back-edge into a cycle (a multi-node
    // component, or a single module importing itself); `seed_use` reports it.
    let components = tarjan(&graph);
    let mut component_of = alloc::vec![0_usize; modules.len()];
    for (c, component) in components.iter().enumerate() {
        for &m in component {
            component_of[m] = c;
        }
    }

    let mut interfaces: Vec<Option<ModuleInterface>> = (0..modules.len()).map(|_| None).collect();
    let mut checked: BTreeMap<ModuleName, CheckedFile> = BTreeMap::new();

    // Components come out callees-first, so a module's out-of-cycle imports are
    // always interfaced before it is checked.
    for component in &components {
        for &i in component {
            let (name, file) = &modules[i];
            let mut checker = Checker::new(u32::try_from(i).unwrap_or(u32::MAX));
            checker.set_module(name.clone());

            for diag in module_name_diagnostics(file, name, i) {
                checker.push_diag(diag);
            }

            for u in &resolved[i] {
                seed_use(&mut checker, u, &interfaces, &component_of, i);
            }

            let (file_result, interface) = checker.run_with_interface(file);
            interfaces[i] = Some(interface);
            checked.insert(name.clone(), file_result);
        }
    }

    CheckedProgram { modules: checked }
}

/// Resolves a file's `use` declarations against the program `index`, attaching
/// spans tagged with `source_id`.
fn resolve_uses(
    file: &SourceFile,
    index: &BTreeMap<&str, usize>,
    source_id: usize,
) -> Vec<ResolvedUse> {
    let id = u32::try_from(source_id).unwrap_or(u32::MAX);
    let mut uses = Vec::new();
    for decl in file.declarations() {
        let Decl::Use(use_decl) = decl else { continue };
        let Some(path) = use_decl.path() else {
            continue;
        };
        let target = ModuleName::new(path.segments().collect::<Vec<_>>().join("."));
        let target_index = index.get(target.as_str()).copied();
        uses.push(ResolvedUse {
            target,
            target_index,
            use_span: node_span(use_decl.syntax(), id),
            path_span: node_span(path.syntax(), id),
            alias: use_decl.alias().map(String::from),
            selected: selected_members(&use_decl, id),
        });
    }
    uses
}

/// The selective-group members of `use_decl` with their name-token spans.
fn selected_members(use_decl: &UseDecl, source_id: u32) -> Vec<(String, Span)> {
    use_decl
        .selected_tokens()
        .map(|t| (String::from(t.text()), token_span(t, source_id)))
        .collect()
}

/// Diagnostics for a `module` declaration that disagrees with the file's
/// path-derived name. An absent declaration is permitted.
fn module_name_diagnostics(
    file: &SourceFile,
    expected: &ModuleName,
    source_id: usize,
) -> Vec<CheckDiagnostic> {
    let id = u32::try_from(source_id).unwrap_or(u32::MAX);
    let Some(decl) = file.module() else {
        return Vec::new();
    };
    let Some(declared) = decl.name() else {
        return Vec::new();
    };
    if declared == expected.as_str() {
        return Vec::new();
    }
    alloc::vec![CheckDiagnostic::error(
        CheckCode::C0019,
        crate::name_token_span(decl.syntax(), id),
        format!("module is declared `{declared}` but its path names it `{expected}`"),
    )]
}

/// Seeds one resolved `use` into `checker`, drawing on already-checked
/// `interfaces`. Imports that close a cycle are reported (not seeded);
/// unresolved modules are reported; selective members that the target does not
/// export are reported.
fn seed_use(
    checker: &mut Checker,
    u: &ResolvedUse,
    interfaces: &[Option<ModuleInterface>],
    component_of: &[usize],
    importer: usize,
) {
    let Some(target_index) = u.target_index else {
        checker.push_diag(CheckDiagnostic::error(
            CheckCode::C0023,
            u.path_span,
            format!("unresolved import: no module named `{}`", u.target),
        ));
        return;
    };

    // A target in the importer's own component is a back-edge: checking it has
    // not produced an interface yet, so the import is part of a cycle.
    if component_of[target_index] == component_of[importer] {
        checker.push_diag(CheckDiagnostic::error(
            CheckCode::C0020,
            u.use_span,
            format!(
                "circular import: module `{}` is part of an import cycle",
                u.target
            ),
        ));
        return;
    }

    let interface = interfaces[target_index]
        .as_ref()
        .expect("dependency checked before dependant");

    if u.selected.is_empty() {
        checker.seed_module_qualifier(u.qualifier(), interface.exported_values());
        return;
    }

    for (member, span) in &u.selected {
        let mut found = false;
        let member_name = Name::new(member.as_str());
        if let Some(exported) = interface.types.get(&member_name) {
            checker.seed_import_type(&member_name, exported, u.target.clone(), *span);
            found = true;
        }
        if let Some(expansion) = interface.aliases.get(&member_name) {
            checker.seed_import_alias(&member_name, expansion.clone(), *span);
            found = true;
        }
        if let Some(scheme) = interface.functions.get(member) {
            checker.seed_import_function(member, scheme.clone(), u.target.clone(), *span);
            found = true;
        } else if let Some((owner, scheme)) = interface.public_ctor(member) {
            checker.seed_import_ctor(member, owner, scheme, u.target.clone(), *span);
            found = true;
        }
        if !found {
            checker.push_diag(CheckDiagnostic::error(
                CheckCode::C0023,
                *span,
                format!("module `{}` does not export `{member}`", u.target),
            ));
        }
    }
}

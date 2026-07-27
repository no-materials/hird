// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The compiler pipeline behind every subcommand: load `.hird` sources,
//! parse, type-check as a program, and lower to IR.

use std::fs;
use std::path::{Path, PathBuf};

use hird_ast::{AstNode, SourceFile};
use hird_check::{CheckedFile, ModuleName};
use hird_ir::IrModule;

use crate::report;
use crate::{Failure, fail};

/// One `.hird` source: its path, path-derived module name, and text.
pub(crate) struct SourceModule {
    /// The source file's path, as given or discovered.
    pub(crate) path: PathBuf,
    /// The path-derived module name (authoritative per ADR-010).
    pub(crate) name: String,
    /// The source text.
    pub(crate) source: String,
}

/// A parsed and type-checked module, ready for lowering.
pub(crate) struct CheckedModule {
    /// The source file's path.
    pub(crate) path: PathBuf,
    /// The path-derived module name.
    pub(crate) name: String,
    /// The parsed source file.
    pub(crate) file: SourceFile,
    /// The checker's side tables for this module.
    pub(crate) checked: CheckedFile,
}

impl CheckedModule {
    /// Lowers the module to IR.
    pub(crate) fn lower(&self) -> IrModule {
        hird_ir::lower_module(&self.file, &self.checked, &self.name)
    }
}

/// Loads `input`: a `.hird` file, or a directory whose `.hird` files each
/// become an independent module (in file-name order).
pub(crate) fn load(input: &Path) -> Result<Vec<SourceModule>, Failure> {
    let mut modules = Vec::new();
    if input.is_dir() {
        let mut paths: Vec<PathBuf> = fs::read_dir(input)
            .map_err(|e| fail!("cannot read directory `{}`: {e}", input.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "hird"))
            .collect();
        if paths.is_empty() {
            return Err(fail!("no .hird files in `{}`", input.display()));
        }
        paths.sort();
        for path in paths {
            modules.push(load_file(&path)?);
        }
    } else {
        modules.push(load_file(input)?);
    }
    let mut seen: std::collections::BTreeMap<&str, &Path> = std::collections::BTreeMap::new();
    for module in &modules {
        if let Some(first) = seen.insert(&module.name, &module.path) {
            return Err(fail!(
                "`{}` and `{}` both derive module name `{}`",
                first.display(),
                module.path.display(),
                module.name
            ));
        }
    }
    Ok(modules)
}

/// Loads one source file, deriving its module name from the file stem.
fn load_file(path: &Path) -> Result<SourceModule, Failure> {
    let source =
        fs::read_to_string(path).map_err(|e| fail!("cannot read `{}`: {e}", path.display()))?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| fail!("`{}` has no usable file name", path.display()))?;
    Ok(SourceModule {
        path: path.to_path_buf(),
        name: module_name(stem),
        source,
    })
}

/// The module name a file stem derives: each `_`/`-`-separated segment is
/// capitalized and the segments concatenated (`repo_utils` → `RepoUtils`).
fn module_name(stem: &str) -> String {
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

/// Parses and type-checks `modules` as one program. Parse and check
/// diagnostics render to stderr; any error fails the pipeline. The returned
/// modules keep the input order.
pub(crate) fn parse_and_check(modules: Vec<SourceModule>) -> Result<Vec<CheckedModule>, Failure> {
    let mut parsed_files = Vec::new();
    let mut parse_errors = false;
    for (source_id, module) in modules.iter().enumerate() {
        let parsed = hird_parse::parse(&module.source, id_of(source_id));
        for diagnostic in parsed.diagnostics() {
            parse_errors = true;
            eprintln!("{}: parse error", module.path.display());
            eprint!(
                "{}",
                hird_parse::diagnostic::render(diagnostic, &module.source)
            );
        }
        let file = SourceFile::cast(parsed.syntax().clone())
            .ok_or_else(|| fail!("`{}`: no source file produced", module.path.display()))?;
        parsed_files.push(file);
    }
    if parse_errors {
        return Err(Failure::Reported);
    }

    let program: Vec<(ModuleName, SourceFile)> = modules
        .iter()
        .zip(&parsed_files)
        .map(|(m, f)| (ModuleName::new(m.name.clone()), f.clone()))
        .collect();
    let mut checked_program = hird_check::check_program(&program);

    let mut errors = false;
    for module in checked_program.modules.values() {
        for diagnostic in &module.diagnostics {
            errors |= diagnostic.severity == hird_check::Severity::Error;
            let source_id = usize::try_from(diagnostic.span.source_id).unwrap_or(usize::MAX);
            match modules.get(source_id) {
                Some(m) => eprint!("{}", report::render(diagnostic, &m.path, &m.source)),
                None => eprintln!("{:?}: {}", diagnostic.code, diagnostic.message),
            }
        }
    }
    if errors {
        return Err(Failure::Reported);
    }

    modules
        .into_iter()
        .zip(parsed_files)
        .map(|(module, file)| {
            let checked = checked_program
                .modules
                .remove(&ModuleName::new(module.name.clone()))
                .ok_or_else(|| {
                    fail!(
                        "`{}`: module `{}` was not checked",
                        module.path.display(),
                        module.name
                    )
                })?;
            Ok(CheckedModule {
                path: module.path,
                name: module.name,
                file,
                checked,
            })
        })
        .collect()
}

/// The `u32` source id of slice index `i` (the `check_program` convention).
fn id_of(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The in-process front end, one stage at a time: lex, parse, check,
//! lower, emit.

use std::time::{Duration, Instant};

use hird_ast::{AstNode, SourceFile};
use hird_check::ModuleName;

use crate::corpus::SourceModule;
use crate::rss;

/// Front-end stage names, in pipeline order.
pub(crate) const STAGES: [&str; 5] = ["lex", "parse", "check", "lower", "emit"];

/// One stage of one pass.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sample {
    /// Wall time.
    pub(crate) time: Duration,
    /// Peak RSS in bytes during the stage, where measurable.
    pub(crate) peak: Option<u64>,
}

/// One pass over a corpus.
#[derive(Debug)]
pub(crate) struct Pass {
    /// Per stage, in [`STAGES`] order.
    pub(crate) stages: [Sample; 5],
    /// Tokens lexed.
    pub(crate) tokens: usize,
    /// Erlang modules emitted.
    pub(crate) erlang_modules: usize,
}

/// Runs every stage once over `modules`, whose paths (for emitted
/// banners) are `paths`. Fails on any parse or check diagnostic, warnings
/// included: a generated corpus must check cleanly.
pub(crate) fn pass(modules: &[SourceModule], paths: &[String]) -> Result<Pass, String> {
    let (tokens, lex) = measure(|| {
        modules
            .iter()
            .enumerate()
            .map(|(id, m)| hird_lex::Lexer::new(&m.source, source_id(id)).count())
            .sum::<usize>()
    });

    let (parsed, parse) = measure(|| {
        modules
            .iter()
            .enumerate()
            .map(|(id, m)| hird_parse::parse(&m.source, source_id(id)))
            .collect::<Vec<_>>()
    });
    let mut problems = Vec::new();
    let mut program = Vec::with_capacity(modules.len());
    for (module, result) in modules.iter().zip(&parsed) {
        for d in result.diagnostics() {
            problems.push(format!(
                "{} at byte {}: {:?} {}",
                module.file, d.span.start, d.code, d.message
            ));
        }
        let file = SourceFile::cast(result.syntax().clone())
            .ok_or_else(|| format!("{}: no source file produced", module.file))?;
        program.push((ModuleName::new(module.name.clone()), file));
    }
    if !problems.is_empty() {
        return Err(format!(
            "the corpus has parse errors:\n{}",
            problems.join("\n")
        ));
    }

    let (checked, check) = measure(|| hird_check::check_program(&program));
    for d in checked.modules.values().flat_map(|m| &m.diagnostics) {
        let file = usize::try_from(d.span.source_id)
            .ok()
            .and_then(|id| modules.get(id))
            .map_or("?", |m| m.file.as_str());
        problems.push(format!(
            "{file} at byte {}: {:?} {:?} {}",
            d.span.start, d.severity, d.code, d.message
        ));
    }
    if !problems.is_empty() {
        return Err(format!(
            "the corpus does not check cleanly:\n{}",
            problems.join("\n")
        ));
    }

    let (irs, lower) = measure(|| {
        program
            .iter()
            .map(|(name, file)| hird_ir::lower_module(file, &checked.modules[name], name.as_str()))
            .collect::<Vec<_>>()
    });

    let (emitted, emit) = measure(|| {
        irs.iter()
            .zip(paths)
            .map(|(ir, path)| hird_codegen::emit_modules(ir, path))
            .collect::<Vec<_>>()
    });

    Ok(Pass {
        stages: [lex, parse, check, lower, emit],
        tokens,
        erlang_modules: emitted.iter().map(Vec::len).sum(),
    })
}

/// Runs `f`, timing it and taking the peak RSS it reached.
fn measure<T>(f: impl FnOnce() -> T) -> (T, Sample) {
    let reset = rss::reset_peak();
    let start = Instant::now();
    let out = f();
    let time = start.elapsed();
    let peak = if reset { rss::peak() } else { None };
    (out, Sample { time, peak })
}

/// The `u32` source id of module index `i`.
fn source_id(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Human-readable rendering of the actor/effect graph (the
//! `emit-effect-graph` default; `--json` serializes the same projection)
//! and of an effect-diff report.

use std::fmt::Write as _;
use std::path::Path;

use hird_ir::{EffectGraph, EffectRowRef};
use hird_policy::{GraphDiff, Severity};

/// Renders `graph` as indented text, locating nodes by `file:line`, where
/// `file` is the final component of `path`: the module's own file name, not
/// a checkout-specific path.
pub(crate) fn render_graph(graph: &EffectGraph, path: &Path) -> String {
    let mut out = String::new();
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    let at = |line: u32| {
        if line > 0 {
            format!("  ({file}:{line})")
        } else {
            String::new()
        }
    };
    let _ = writeln!(out, "module {}", graph.module);
    for actor in &graph.actors {
        let _ = writeln!(out);
        let _ = writeln!(out, "actor {}{}", actor.name, at(actor.line));
        let _ = writeln!(out, "  state {}", actor.state.display);
        let _ = writeln!(out, "  message {} =", actor.message.name);
        for ctor in &actor.message.constructors {
            let fields: Vec<&str> = ctor.fields.iter().map(|f| f.display.as_str()).collect();
            if fields.is_empty() {
                let _ = writeln!(out, "    {}", ctor.name);
            } else {
                let _ = writeln!(out, "    {}({})", ctor.name, fields.join(", "));
            }
        }
        let params: Vec<String> = actor
            .init
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ty.display))
            .collect();
        let _ = writeln!(
            out,
            "  init({}) ! {}",
            params.join(", "),
            actor.init.effects.display
        );
        for handler in &actor.handlers {
            let _ = writeln!(
                out,
                "  handle {} ! {}",
                handler.message, handler.effects.display
            );
        }
        let _ = writeln!(out, "  effects {}", actor.effects.display);
    }
    for sup in &graph.supervisors {
        let _ = writeln!(out);
        let _ = writeln!(out, "supervisor {}{}", sup.name, at(sup.line));
        let _ = writeln!(
            out,
            "  strategy {} (intensity {}, period {})",
            sup.strategy, sup.intensity, sup.period
        );
        for child in &sup.children {
            let _ = writeln!(
                out,
                "  child {}: {} ({})",
                child.id, child.actor, child.restart
            );
        }
        let _ = writeln!(out, "  effects {}", sup.effects.display);
    }
    for tool in &graph.tools {
        let _ = writeln!(out);
        let params = if tool.params.is_empty() {
            String::new()
        } else {
            format!("<{}>", tool.params.join(", "))
        };
        let _ = writeln!(
            out,
            "tool {}{} : {} \u{2192} {}{}{}",
            tool.name,
            params,
            tool.input.display,
            tool.output.display,
            row_suffix(&tool.effects),
            at(tool.line)
        );
    }
    for f in &graph.functions {
        let _ = writeln!(out);
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ty.display))
            .collect();
        let _ = writeln!(
            out,
            "fn {}({}) \u{2192} {}{}{}",
            f.name,
            params.join(", "),
            f.result.display,
            row_suffix(&f.effects),
            at(f.line)
        );
    }
    out
}

/// ` ! {row}` for a declared row; empty for a pure, closed one.
fn row_suffix(row: &EffectRowRef) -> String {
    if row.effects.is_empty() && !row.open {
        String::new()
    } else {
        format!(" ! {}", row.display)
    }
}

/// Renders `diff` as one line per change, then a count and the verdict.
pub(crate) fn render_diff(diff: &GraphDiff) -> String {
    let mut out = String::new();
    for change in &diff.changes {
        let _ = writeln!(out, "{:<8} {change}", change.severity.to_string());
    }
    if diff.changes.is_empty() {
        let _ = writeln!(out, "no changes");
    } else {
        let count = |severity: Severity| {
            diff.changes
                .iter()
                .filter(|c| c.severity == severity)
                .count()
        };
        let _ = writeln!(
            out,
            "\n{} widened, {} narrowed, {} changed",
            count(Severity::Widened),
            count(Severity::Narrowed),
            count(Severity::Changed)
        );
    }
    let _ = writeln!(
        out,
        "{}",
        if diff.widens {
            "effect reach widened"
        } else {
            "effect reach not widened"
        }
    );
    out
}

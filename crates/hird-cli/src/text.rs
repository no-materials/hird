// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Human-readable rendering of the actor/effect graph (the
//! `emit-effect-graph` default; `--json` serializes the same projection).

use std::fmt::Write as _;
use std::path::Path;

use hird_ir::EffectGraph;

/// Renders `graph` as indented text, locating nodes by `path:line`.
pub(crate) fn render_graph(graph: &EffectGraph, path: &Path) -> String {
    let mut out = String::new();
    let at = |line: u32| {
        if line > 0 {
            format!("  ({}:{line})", path.display())
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
        let row = if tool.effects.effects.is_empty() && !tool.effects.open {
            String::new()
        } else {
            format!(" ! {}", tool.effects.display)
        };
        let _ = writeln!(
            out,
            "tool {}{} : {} \u{2192} {}{}{}",
            tool.name,
            params,
            tool.input.display,
            tool.output.display,
            row,
            at(tool.line)
        );
    }
    out
}

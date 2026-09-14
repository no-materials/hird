// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Effect-graph policy: diffing a committed baseline against a fresh
//! [`ProgramGraph`] and classifying every change as widening, narrowing,
//! or neutral. Widening — anything that lets the program reach more than
//! the baseline allowed — is what a CI gate fails on.
//!
//! The diff follows the graph's identity contract: declarations match by
//! module and name (handlers by message, children by id), and `line` and
//! declaration order never register.
//!
//! # Severity
//!
//! - [`Severity::Widened`]: a row gains an effect or opens; a constructor
//!   is added, removed, or changes shape (the mailbox protocol moved); an
//!   actor or tool is added; a function or supervisor is added with a
//!   non-empty or open row.
//! - [`Severity::Narrowed`]: a row loses an effect or closes; a declaration
//!   or module is removed.
//! - [`Severity::Changed`]: everything else — a type in a signature, a
//!   supervisor's strategy or budget, its child set, a pure declaration
//!   added, a module added.
//!
//! The report is itself a versioned JSON document ([`DIFF_SCHEMA_VERSION`])
//! and evolves additively only.

#![no_std]

extern crate alloc;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use hird_ir::{
    ActorNode, ConstructorNode, EffectGraph, EffectRowRef, FnNode, ParamNode, ProgramGraph,
    SupervisorNode, ToolNode,
};
use serde::{Deserialize, Serialize};

/// Version of the diff-report schema. Bumped only for breaking changes.
pub const DIFF_SCHEMA_VERSION: u32 = 1;

/// The classified differences between a baseline graph and a current one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphDiff {
    /// Schema version of this report ([`DIFF_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// Whether any change is [`Severity::Widened`]: the gate's verdict.
    pub widens: bool,
    /// Every change, ordered by module, node kind, then name.
    pub changes: Vec<Change>,
}

/// One difference, located at a declaration of one module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// The module the declaration lives in.
    pub module: String,
    /// What kind of declaration changed.
    pub kind: NodeKind,
    /// The declaration's name (the module's own name for [`NodeKind::Module`]).
    pub name: String,
    /// How the change bears on effect reach.
    pub severity: Severity,
    /// What changed.
    pub detail: Detail,
}

/// The kind of graph node a change is located at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A whole module.
    Module,
    /// An actor declaration.
    Actor,
    /// A supervisor declaration.
    Supervisor,
    /// A tool declaration.
    Tool,
    /// A plain function declaration.
    Function,
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Module => "module",
            Self::Actor => "actor",
            Self::Supervisor => "supervisor",
            Self::Tool => "tool",
            Self::Function => "function",
        })
    }
}

/// How a change bears on what the program may reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Reach grew; a gate fails on this.
    Widened,
    /// Reach shrank.
    Narrowed,
    /// Reach is unchanged; the shape moved.
    Changed,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Widened => "widened",
            Self::Narrowed => "narrowed",
            Self::Changed => "changed",
        })
    }
}

/// What changed at a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum Detail {
    /// The node is new.
    Added,
    /// The node is gone.
    Removed,
    /// An effect row differs.
    Row {
        /// Which row: `init`, `handle <Message>`, `summary`, or `row`.
        site: String,
        /// Effects present now and absent in the baseline (canonical displays).
        added: Vec<String>,
        /// Effects present in the baseline and absent now.
        removed: Vec<String>,
        /// The row's new openness, when it changed.
        opened: Option<bool>,
    },
    /// The mailbox protocol differs.
    Constructors {
        /// Constructors present now and absent in the baseline.
        added: Vec<String>,
        /// Constructors present in the baseline and absent now.
        removed: Vec<String>,
        /// Constructors whose field types differ.
        changed: Vec<String>,
    },
    /// A type in the signature differs.
    Type {
        /// Which type: `state`, `init`, or `signature`.
        site: String,
        /// The baseline rendering.
        before: String,
        /// The current rendering.
        after: String,
    },
    /// A supervisor's strategy or restart budget differs.
    Supervision {
        /// `strategy`, `intensity`, or `period`.
        field: String,
        /// The baseline value.
        before: String,
        /// The current value.
        after: String,
    },
    /// A supervisor's child set differs.
    Children {
        /// Child ids present now and absent in the baseline.
        added: Vec<String>,
        /// Child ids present in the baseline and absent now.
        removed: Vec<String>,
        /// Child ids whose actor or restart mode differs.
        changed: Vec<String>,
    },
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}: ", self.module, self.kind, self.name)?;
        match &self.detail {
            Detail::Added => f.write_str("added"),
            Detail::Removed => f.write_str("removed"),
            Detail::Row {
                site,
                added,
                removed,
                opened,
            } => {
                f.write_str(site)?;
                let mut parts = Vec::new();
                if !added.is_empty() {
                    parts.push(format!("gains {}", added.join(", ")));
                }
                if !removed.is_empty() {
                    parts.push(format!("loses {}", removed.join(", ")));
                }
                match opened {
                    Some(true) => parts.push("opens".into()),
                    Some(false) => parts.push("closes".into()),
                    None => {}
                }
                write!(f, " {}", parts.join("; "))
            }
            Detail::Constructors {
                added,
                removed,
                changed,
            } => {
                f.write_str("protocol")?;
                let mut parts = Vec::new();
                if !added.is_empty() {
                    parts.push(format!("gains {}", added.join(", ")));
                }
                if !removed.is_empty() {
                    parts.push(format!("loses {}", removed.join(", ")));
                }
                if !changed.is_empty() {
                    parts.push(format!("reshapes {}", changed.join(", ")));
                }
                write!(f, " {}", parts.join("; "))
            }
            Detail::Type {
                site,
                before,
                after,
            } => write!(f, "{site} {before} \u{2192} {after}"),
            Detail::Supervision {
                field,
                before,
                after,
            } => write!(f, "{field} {before} \u{2192} {after}"),
            Detail::Children {
                added,
                removed,
                changed,
            } => {
                f.write_str("children")?;
                let mut parts = Vec::new();
                if !added.is_empty() {
                    parts.push(format!("gains {}", added.join(", ")));
                }
                if !removed.is_empty() {
                    parts.push(format!("loses {}", removed.join(", ")));
                }
                if !changed.is_empty() {
                    parts.push(format!("changes {}", changed.join(", ")));
                }
                write!(f, " {}", parts.join("; "))
            }
        }
    }
}

/// Diffs `current` against `baseline`.
#[must_use]
pub fn diff(baseline: &ProgramGraph, current: &ProgramGraph) -> GraphDiff {
    let mut out = Vec::new();
    let names: BTreeSet<&String> = baseline
        .modules
        .keys()
        .chain(current.modules.keys())
        .collect();
    for name in names {
        match (baseline.modules.get(name), current.modules.get(name)) {
            (Some(before), Some(after)) => diff_module(before, after, &mut out),
            (None, Some(after)) => {
                out.push(module_change(name, Severity::Changed, Detail::Added));
                diff_module(&empty_module(name), after, &mut out);
            }
            (Some(before), None) => {
                out.push(module_change(name, Severity::Narrowed, Detail::Removed));
                diff_module(before, &empty_module(name), &mut out);
            }
            (None, None) => unreachable!("name comes from one of the two maps"),
        }
    }
    GraphDiff {
        schema_version: DIFF_SCHEMA_VERSION,
        widens: out.iter().any(|c| c.severity == Severity::Widened),
        changes: out,
    }
}

/// A module-level change record.
fn module_change(name: &str, severity: Severity, detail: Detail) -> Change {
    Change {
        module: name.into(),
        kind: NodeKind::Module,
        name: name.into(),
        severity,
        detail,
    }
}

/// A graph with `name` and nothing in it, standing in for an absent module.
fn empty_module(name: &str) -> EffectGraph {
    EffectGraph {
        schema_version: hird_ir::EFFECT_GRAPH_SCHEMA_VERSION,
        module: name.into(),
        actors: Vec::new(),
        supervisors: Vec::new(),
        tools: Vec::new(),
        functions: Vec::new(),
    }
}

/// Diffs the declarations of one module, kind by kind, matching by name.
fn diff_module(before: &EffectGraph, after: &EffectGraph, out: &mut Vec<Change>) {
    let module = before.module.as_str();
    diff_kind(
        module,
        NodeKind::Actor,
        &before.actors,
        &after.actors,
        |a| &a.name,
        |_| Severity::Widened,
        diff_actor,
        out,
    );
    diff_kind(
        module,
        NodeKind::Supervisor,
        &before.supervisors,
        &after.supervisors,
        |s| &s.name,
        |s| added_severity(&s.effects),
        diff_supervisor,
        out,
    );
    diff_kind(
        module,
        NodeKind::Tool,
        &before.tools,
        &after.tools,
        |t| &t.name,
        |_| Severity::Widened,
        diff_tool,
        out,
    );
    diff_kind(
        module,
        NodeKind::Function,
        &before.functions,
        &after.functions,
        |f| &f.name,
        |f| added_severity(&f.effects),
        diff_fn,
        out,
    );
}

/// Widened when a newly added declaration's row reaches anything at all.
fn added_severity(row: &EffectRowRef) -> Severity {
    if row.effects.is_empty() && !row.open {
        Severity::Changed
    } else {
        Severity::Widened
    }
}

/// Matches `before` and `after` by `name`, reporting additions (with the
/// severity `on_add` assigns), removals, and — through `diff_pair` — the
/// changes within each surviving pair.
fn diff_kind<'a, N: 'a>(
    module: &str,
    kind: NodeKind,
    before: &'a [N],
    after: &'a [N],
    name: impl Fn(&N) -> &String,
    on_add: impl Fn(&N) -> Severity,
    diff_pair: impl Fn(&N, &N, &mut Site<'_>),
    out: &mut Vec<Change>,
) {
    let before: BTreeMap<&String, &N> = before.iter().map(|n| (name(n), n)).collect();
    let after: BTreeMap<&String, &N> = after.iter().map(|n| (name(n), n)).collect();
    let names: BTreeSet<&String> = before.keys().chain(after.keys()).copied().collect();
    for n in names {
        let mut site = Site {
            module,
            kind,
            name: n,
            out,
        };
        match (before.get(n), after.get(n)) {
            (Some(b), Some(a)) => diff_pair(b, a, &mut site),
            (None, Some(a)) => site.push(on_add(a), Detail::Added),
            (Some(_), None) => site.push(Severity::Narrowed, Detail::Removed),
            (None, None) => unreachable!("name comes from one of the two maps"),
        }
    }
}

/// Where changes within one declaration are recorded.
struct Site<'a> {
    /// The declaration's module.
    module: &'a str,
    /// The declaration's kind.
    kind: NodeKind,
    /// The declaration's name.
    name: &'a str,
    /// The report under construction.
    out: &'a mut Vec<Change>,
}

impl Site<'_> {
    /// Records one change at this declaration.
    fn push(&mut self, severity: Severity, detail: Detail) {
        self.out.push(Change {
            module: self.module.into(),
            kind: self.kind,
            name: self.name.into(),
            severity,
            detail,
        });
    }

    /// Records a row difference at `site`, if any.
    fn row(&mut self, site: &str, before: &EffectRowRef, after: &EffectRowRef) {
        let b: BTreeSet<&String> = before.effects.iter().map(|e| &e.display).collect();
        let a: BTreeSet<&String> = after.effects.iter().map(|e| &e.display).collect();
        let added: Vec<String> = a.difference(&b).map(|s| (*s).clone()).collect();
        let removed: Vec<String> = b.difference(&a).map(|s| (*s).clone()).collect();
        let opened = (before.open != after.open).then_some(after.open);
        if added.is_empty() && removed.is_empty() && opened.is_none() {
            return;
        }
        let severity = if !added.is_empty() || opened == Some(true) {
            Severity::Widened
        } else {
            Severity::Narrowed
        };
        self.push(
            severity,
            Detail::Row {
                site: site.into(),
                added,
                removed,
                opened,
            },
        );
    }

    /// Records a type difference at `site`, if any.
    fn ty(&mut self, site: &str, before: &str, after: &str) {
        if before != after {
            self.push(
                Severity::Changed,
                Detail::Type {
                    site: site.into(),
                    before: before.into(),
                    after: after.into(),
                },
            );
        }
    }
}

/// Diffs two versions of one actor.
fn diff_actor(before: &ActorNode, after: &ActorNode, site: &mut Site<'_>) {
    site.ty("state", &before.state.display, &after.state.display);
    site.ty(
        "init",
        &params(&before.init.params),
        &params(&after.init.params),
    );
    site.row("init", &before.init.effects, &after.init.effects);
    diff_constructors(
        &before.message.constructors,
        &after.message.constructors,
        site,
    );
    let empty = EffectRowRef {
        display: "{}".into(),
        effects: Vec::new(),
        open: false,
    };
    let b: BTreeMap<&String, &EffectRowRef> = before
        .handlers
        .iter()
        .map(|h| (&h.message, &h.effects))
        .collect();
    let a: BTreeMap<&String, &EffectRowRef> = after
        .handlers
        .iter()
        .map(|h| (&h.message, &h.effects))
        .collect();
    let messages: BTreeSet<&String> = b.keys().chain(a.keys()).copied().collect();
    for message in messages {
        let before_row = b.get(message).copied().unwrap_or(&empty);
        let after_row = a.get(message).copied().unwrap_or(&empty);
        site.row(&format!("handle {message}"), before_row, after_row);
    }
    site.row("summary", &before.effects, &after.effects);
}

/// Records a protocol difference, if any: constructors matched by name,
/// compared by field types.
fn diff_constructors(before: &[ConstructorNode], after: &[ConstructorNode], site: &mut Site<'_>) {
    let b: BTreeMap<&String, &ConstructorNode> = before.iter().map(|c| (&c.name, c)).collect();
    let a: BTreeMap<&String, &ConstructorNode> = after.iter().map(|c| (&c.name, c)).collect();
    let names: BTreeSet<&String> = b.keys().chain(a.keys()).copied().collect();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for name in names {
        match (b.get(name), a.get(name)) {
            (Some(x), Some(y)) => {
                if fields(x) != fields(y) {
                    changed.push(name.clone());
                }
            }
            (None, Some(_)) => added.push(name.clone()),
            (Some(_), None) => removed.push(name.clone()),
            (None, None) => unreachable!("name comes from one of the two maps"),
        }
    }
    if added.is_empty() && removed.is_empty() && changed.is_empty() {
        return;
    }
    site.push(
        Severity::Widened,
        Detail::Constructors {
            added,
            removed,
            changed,
        },
    );
}

/// Diffs two versions of one supervisor.
fn diff_supervisor(before: &SupervisorNode, after: &SupervisorNode, site: &mut Site<'_>) {
    let fields = [
        ("strategy", before.strategy.clone(), after.strategy.clone()),
        (
            "intensity",
            format!("{}", before.intensity),
            format!("{}", after.intensity),
        ),
        (
            "period",
            format!("{}", before.period),
            format!("{}", after.period),
        ),
    ];
    for (field, b, a) in fields {
        if b != a {
            site.push(
                Severity::Changed,
                Detail::Supervision {
                    field: field.into(),
                    before: b,
                    after: a,
                },
            );
        }
    }
    let b: BTreeMap<&String, (&String, &String)> = before
        .children
        .iter()
        .map(|c| (&c.id, (&c.actor, &c.restart)))
        .collect();
    let a: BTreeMap<&String, (&String, &String)> = after
        .children
        .iter()
        .map(|c| (&c.id, (&c.actor, &c.restart)))
        .collect();
    let ids: BTreeSet<&String> = b.keys().chain(a.keys()).copied().collect();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for id in ids {
        match (b.get(id), a.get(id)) {
            (Some(x), Some(y)) => {
                if x != y {
                    changed.push(id.clone());
                }
            }
            (None, Some(_)) => added.push(id.clone()),
            (Some(_), None) => removed.push(id.clone()),
            (None, None) => unreachable!("id comes from one of the two maps"),
        }
    }
    if !(added.is_empty() && removed.is_empty() && changed.is_empty()) {
        site.push(
            Severity::Changed,
            Detail::Children {
                added,
                removed,
                changed,
            },
        );
    }
    site.row("summary", &before.effects, &after.effects);
}

/// Diffs two versions of one tool.
fn diff_tool(before: &ToolNode, after: &ToolNode, site: &mut Site<'_>) {
    site.ty("signature", &tool_signature(before), &tool_signature(after));
    site.row("row", &before.effects, &after.effects);
}

/// Diffs two versions of one function.
fn diff_fn(before: &FnNode, after: &FnNode, site: &mut Site<'_>) {
    site.ty("signature", &fn_signature(before), &fn_signature(after));
    site.row("row", &before.effects, &after.effects);
}

/// `(a: A, b: B)`.
fn params(params: &[ParamNode]) -> String {
    let rendered: Vec<String> = params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty.display))
        .collect();
    format!("({})", rendered.join(", "))
}

/// `(a: A) → R`.
fn fn_signature(f: &FnNode) -> String {
    format!("{} \u{2192} {}", params(&f.params), f.result.display)
}

/// `<t> : In → Out` (the type-parameter list only when present).
fn tool_signature(t: &ToolNode) -> String {
    let generics = if t.params.is_empty() {
        String::new()
    } else {
        format!("<{}> ", t.params.join(", "))
    };
    format!(
        "{generics}: {} \u{2192} {}",
        t.input.display, t.output.display
    )
}

/// `(A, B)` — a constructor's field types.
fn fields(c: &ConstructorNode) -> Vec<&String> {
    c.fields.iter().map(|f| &f.display).collect()
}

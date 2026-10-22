// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Work counters (`CheckedFile::stats`): what each one counts, that they
//! are deterministic, and that a program's total sums its modules.

use hird_ast::{AstNode, SourceFile};
use hird_check::{CheckStats, CheckedFile, ModuleName, check_program};

/// A pure function with no match.
const PURE: &str = "fn add(x: Int, y: Int) → Int = x + y";

/// One body calling three tools.
const TOOLS: &str = "tool A : Int → Int\n\
     tool B : Int → Int\n\
     tool C : Int → Int\n\
     fn f(x: Int) → Int ! {Tool<A>, Tool<B>, Tool<C>} = a(x) + b(x) + c(x)";

/// A match over a closed type.
const MATCH: &str = "type Shape = Circle(Int) | Square(Int) | Empty\n\
     fn area(s: Shape) → Int = match s { Circle(r) → r * r, Square(w) → w, _ → 0 }";

/// The parsed file of `source`.
fn parse(source: &str) -> SourceFile {
    let parsed = hird_parse::parse(source, 0);
    assert!(parsed.is_ok(), "parse errors: {:?}", parsed.diagnostics());
    SourceFile::cast(parsed.syntax().clone()).expect("root is a source file")
}

/// Checks `source`, which must check cleanly.
fn check(source: &str) -> CheckedFile {
    let checked = hird_check::check(&parse(source), 0);
    assert!(
        checked.diagnostics.is_empty(),
        "diagnostics: {:?}",
        checked.diagnostics
    );
    checked
}

#[test]
fn typed_nodes_is_the_type_table_size() {
    let checked = check(MATCH);
    assert_eq!(checked.stats.typed_nodes, checked.types.len() as u64);
    assert!(checked.stats.unify_calls > 0, "{:?}", checked.stats);
    assert!(checked.stats.subst_slots > 0, "{:?}", checked.stats);
}

#[test]
fn only_matches_and_effects_count_their_work() {
    let pure = check(PURE).stats;
    assert_eq!(
        (
            pure.exhaustiveness_rows,
            pure.exhaustiveness_witnesses,
            pure.effect_row_merges
        ),
        (0, 0, 0),
        "{pure:?}"
    );
    let matched = check(MATCH).stats;
    assert!(matched.exhaustiveness_rows > 0, "{matched:?}");
    assert!(matched.exhaustiveness_witnesses > 0, "{matched:?}");
    assert_eq!(matched.effect_row_merges, 0, "{matched:?}");
}

#[test]
fn effect_row_merges_revisit_the_accumulator() {
    // Each call's one effect, plus the effects already accumulated.
    assert_eq!(check(TOOLS).stats.effect_row_merges, 1 + 2 + 3);
}

#[test]
fn counters_are_deterministic() {
    for source in [PURE, TOOLS, MATCH] {
        assert_eq!(check(source).stats, check(source).stats, "{source}");
    }
}

/// A match on a `width`-wide tuple of `Bool`s: one all-`True` row, then `_`.
fn tuple_match(width: usize) -> String {
    let params: Vec<String> = (1..=width).map(|i| format!("a{i}: Bool")).collect();
    let args: Vec<String> = (1..=width).map(|i| format!("a{i}")).collect();
    format!(
        "fn f({}) → Int = match ({}) {{ ({}) → 1, _ → 0 }}",
        params.join(", "),
        args.join(", "),
        vec!["True"; width].join(", ")
    )
}

/// A decision table: ten rows over a `width`-wide tuple of a ten-constructor
/// enum, each column a rotation of the constructors, then `_`.
fn decision_table(width: usize) -> String {
    let params: Vec<String> = (1..=width).map(|i| format!("a{i}: Level")).collect();
    let args: Vec<String> = (1..=width).map(|i| format!("a{i}")).collect();
    let rows: Vec<String> = (0..10)
        .map(|row| {
            let cells: Vec<String> = (0..width)
                .map(|col| format!("L{}", (row + col) % 10 + 1))
                .collect();
            format!("({}) → {row}", cells.join(", "))
        })
        .collect();
    format!(
        "type Level = L1 | L2 | L3 | L4 | L5 | L6 | L7 | L8 | L9 | L10\n\
         fn f({}) → Int = match ({}) {{ {}, _ → 0 }}",
        params.join(", "),
        args.join(", "),
        rows.join(", ")
    )
}

#[test]
fn exhaustiveness_work_is_linear_in_tuple_width() {
    for (shape, small, large) in [
        (
            "tuple-match",
            check(&tuple_match(6)),
            check(&tuple_match(12)),
        ),
        (
            "decision-table",
            check(&decision_table(2)),
            check(&decision_table(4)),
        ),
    ] {
        let (small, large) = (small.stats, large.stats);
        assert!(
            large.exhaustiveness_rows <= 2 * small.exhaustiveness_rows,
            "{shape}: {small:?} vs {large:?}"
        );
        assert!(
            large.exhaustiveness_witnesses <= 2 * small.exhaustiveness_witnesses,
            "{shape}: {small:?} vs {large:?}"
        );
    }
}

#[test]
fn program_stats_sum_the_modules() {
    let program = check_program(&[
        (ModuleName::new("Tools"), parse(TOOLS)),
        (ModuleName::new("Shapes"), parse(MATCH)),
    ]);
    assert!(!program.has_errors());
    let mut sum = CheckStats::default();
    for module in program.modules.values() {
        sum += module.stats;
    }
    assert_eq!(program.stats(), sum);
    assert_eq!(
        program.stats().effect_row_merges,
        check(TOOLS).stats.effect_row_merges
    );
}

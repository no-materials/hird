// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

use std::fmt::Write;

use hird_ast::{AstNode, SourceFile};
use hird_check::Severity;

/// Parses, checks, and renders `source` as resolved top-level bindings,
/// derived invocation records, and diagnostics.
fn check_str(source: &str) -> String {
    let parsed = hird_parse::parse(source, 0);
    assert!(
        parsed.is_ok(),
        "test source has parse errors: {:?}",
        parsed.diagnostics()
    );
    let file = SourceFile::cast(parsed.syntax().clone()).expect("root is a source file");
    let checked = hird_check::check(&file, 0);
    let mut out = String::new();
    for (name, ty) in &checked.bindings {
        writeln!(out, "{name} : {}", ty.normalized()).unwrap();
    }
    for (name, ty) in &checked.invocation_records {
        writeln!(out, "record {name} : {}", ty.normalized()).unwrap();
    }
    for diag in &checked.diagnostics {
        let severity = match diag.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        writeln!(
            out,
            "{severity}[{:?}] {}..{}: {}",
            diag.code, diag.span.start, diag.span.end, diag.message
        )
        .unwrap();
    }
    out
}

// ── declarations ────────────────────────────────────────────────

/// A tool declaration binds a `snake_case` function whose row carries
/// `Tool<Name>` and derives a `{ tool, args, result, timestamp, caller }`
/// invocation record under the generated `NameInvocation` name.
#[test]
fn tool_declares_function_and_record() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         type Path = Path(String)\n\
         type RepoState = RepoState(String)\n\
         tool ReadRepo : { path: Path } -> RepoState"
    ));
}

/// A generic tool binds its type parameters in a closed scope and
/// generalises; the trailing row unions with the tool's own effect.
#[test]
fn generic_tool_generalises_with_trailing_row() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         effect Exn<t>\n\
         type Prompt = Prompt(String)\n\
         type Schema<t> = Schema(String)\n\
         type ParseError = ParseError(String)\n\
         tool LLMCall<t> : { prompt: Prompt, schema: Schema<t> } -> t ! {Exn<ParseError>}"
    ));
}

/// The invocation-record accessor resolves the generated name.
#[test]
fn invocation_record_accessor() {
    let parsed = hird_parse::parse(
        "effect Tool<t>\n\
         type Path = Path(String)\n\
         type RepoState = RepoState(String)\n\
         tool ReadRepo : { path: Path } -> RepoState",
        0,
    );
    let file = SourceFile::cast(parsed.syntax().clone()).expect("root is a source file");
    let checked = hird_check::check(&file, 0);
    let record = checked
        .invocation_record("ReadRepoInvocation")
        .expect("derived record is registered");
    assert_eq!(
        record.to_string(),
        "{ args: { path: Path }, caller: CallerId, result: RepoState, \
         timestamp: Timestamp, tool: String }"
    );
    assert!(checked.invocation_record("ReadRepo").is_none());
}

// ── calls and effect rows ───────────────────────────────────────

/// Calling a tool function performs `Tool<Name>`, which the caller must
/// declare.
#[test]
fn tool_call_in_effect_row() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         type Path = Path(String)\n\
         type RepoState = RepoState(String)\n\
         tool ReadRepo : { path: Path } -> RepoState\n\
         fn plan(p: Path) -> RepoState ! {Tool<ReadRepo>} = read_repo({ path: p })"
    ));
}

/// A caller that omits the tool effect from its declared row is rejected.
#[test]
fn undeclared_tool_effect_is_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         type Path = Path(String)\n\
         type RepoState = RepoState(String)\n\
         tool ReadRepo : { path: Path } -> RepoState\n\
         fn plan(p: Path) -> RepoState = read_repo({ path: p })"
    ));
}

/// Calling a generic tool instantiates its result from the schema argument;
/// the caller declares the tool effect and the trailing row's effects.
#[test]
fn generic_tool_call_instantiates_result() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         effect Exn<t>\n\
         type Prompt = Prompt(String)\n\
         type Schema<t> = Schema(String)\n\
         type ParseError = ParseError(String)\n\
         tool LLMCall<t> : { prompt: Prompt, schema: Schema<t> } -> t ! {Exn<ParseError>}\n\
         fn ask(p: Prompt, s: Schema<Int>) -> Int ! {Exn<ParseError>, Tool<LLMCall>} =\n\
           llm_call({ prompt: p, schema: s })"
    ));
}

/// A handle arm substitutes a tool implementation, removing the tool effect
/// from the block's row: the enclosing function is pure.
#[test]
fn handle_substitutes_tool_effect() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         type Path = Path(String)\n\
         type RepoState = RepoState(String)\n\
         tool ReadRepo : { path: Path } -> RepoState\n\
         fn mock(args: { path: Path }) -> RepoState = RepoState(\"clean\")\n\
         fn dry_run(p: Path) -> RepoState = handle { Tool<ReadRepo> -> mock } in read_repo({ path: p })"
    ));
}

// ── standard library ────────────────────────────────────────────

/// The standard tools (`llm_call`, `http_get`, `http_post`, `read_file`,
/// `write_file`, `shell`) and their supporting types, declared in a fixture
/// until stdlib resolution lands.
#[test]
fn standard_library_tools() {
    insta::assert_snapshot!(check_str(include_str!("fixtures/std_tools.hird")));
}

// ── errors ──────────────────────────────────────────────────────

/// A trailing row naming an undeclared effect is rejected.
#[test]
fn unknown_effect_in_tool_row_is_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         tool Risky : { input: String } -> String ! {Nope}"
    ));
}

/// `Tool` is a single arity-1 effect; applying it to two arguments in an
/// annotation is rejected.
#[test]
fn tool_effect_wrong_arity_is_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         type Path = Path(String)\n\
         type RepoState = RepoState(String)\n\
         tool ReadRepo : { path: Path } -> RepoState\n\
         fn plan(p: Path) -> RepoState ! {Tool<ReadRepo, Path>} = read_repo({ path: p })"
    ));
}

/// A tool occupies both namespaces: its marker collides with a like-named
/// type, its generated function with a like-named `fn`.
#[test]
fn tool_name_collisions_are_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         type Fetch = MkFetch(Int)\n\
         tool Fetch : { x: Int } -> Int\n\
         fn fetch(x: Int) -> Int = x"
    ));
}

/// Duplicate type parameters on a generic tool are rejected, consistent with
/// ADT headers.
#[test]
fn generic_tool_duplicate_param_is_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         tool Wrap<t, t> : { value: t } -> t"
    ));
}

/// Tool signatures elaborate in a closed scope: an undeclared row variable in
/// the trailing row is rejected.
#[test]
fn open_row_in_tool_decl_is_rejected() {
    insta::assert_snapshot!(check_str(
        "effect Tool<t>\n\
         tool Leaky : { x: Int } -> Int ! {r}"
    ));
}

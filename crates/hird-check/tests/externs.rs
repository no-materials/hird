// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

use std::fmt::Write;

use hird_ast::{AstNode, SourceFile};
use hird_check::Severity;

/// Parses, checks, and renders `source` as resolved top-level bindings and
/// diagnostics.
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

/// A file declaring one Erlang source, an error type, and a tool that may
/// raise it, followed by `rest`.
fn with_tool(rest: &str) -> String {
    format!(
        "extern module \"ffi/files.erl\"\n\
         type Missing = Missing(String)\n\
         tool ReadFile : {{ path: String }} -> String ! {{Exn<Missing>}}\n\
         {rest}"
    )
}

// ── accepted ────────────────────────────────────────────────────

/// A bound extern installed for its tool checks: its `Exn` is the tool's,
/// so the pure-handler rule admits it.
#[test]
fn an_extern_installs_as_its_tool_s_handler() {
    insta::assert_snapshot!(check_str(&with_tool(
        "extern fn erl_read_file(args: { path: String }) -> String ! {Exn<Missing>}\n\
           = \"files\":\"read\"\n\
         fn main() -> () ! {Install} =\n\
           install { Tool<ReadFile> -> erl_read_file } in ()"
    )));
}

/// An extern also handles a tool lexically, and one that never raises may
/// handle a tool that may.
#[test]
fn an_extern_handles_a_tool_in_a_handle_arm() {
    insta::assert_snapshot!(check_str(&with_tool(
        "extern fn erl_read_file(args: { path: String }) -> String = \"files\":\"read\"\n\
         fn read(p: String) -> String ! {Exn<Missing>} =\n\
           handle { Tool<ReadFile> -> erl_read_file } in read_file({ path: p })"
    )));
}

/// A generic extern handles a generic tool, matched by instantiation.
#[test]
fn a_generic_extern_handles_a_generic_tool() {
    insta::assert_snapshot!(check_str(
        "extern module \"echo.erl\"\n\
         tool Echo<t> : { value: t } -> t\n\
         extern fn erl_echo(args: { value: t }) -> t = \"echo\":\"echo\"\n\
         fn main() -> () ! {Install} = install { Tool<Echo> -> erl_echo } in ()"
    ));
}

/// A local binding that shadows an extern's name is an ordinary value.
#[test]
fn a_shadowing_local_is_not_the_extern() {
    insta::assert_snapshot!(check_str(&with_tool(
        "extern fn erl_read_file(args: { path: String }) -> String = \"files\":\"read\"\n\
         fn f(erl_read_file: Int) -> Int = erl_read_file + 1"
    )));
}

// ── C0064: the binding ──────────────────────────────────────────

#[test]
fn an_extern_must_name_its_erlang_function() {
    insta::assert_snapshot!(check_str(
        "extern module \"files.erl\"\n\
         extern fn unbound(path: String) -> String\n\
         extern fn shouting(path: String) -> String = \"files\":\"Read\"\n\
         extern fn escaped(path: String) -> String = \"files\":\"re\\\\ad\""
    ));
}

// ── C0065: Erlang sources ───────────────────────────────────────

#[test]
fn extern_module_paths_are_relative_erl_files_naming_plain_modules() {
    insta::assert_snapshot!(check_str(
        "extern module \"/abs/files.erl\"\n\
         extern module \"files.ex\"\n\
         extern module \"ffi//files.erl\"\n\
         extern module \"Files.erl\"\n\
         extern module \"hird_files.erl\"\n\
         extern module \"ok.erl\"\n\
         extern module \"other/ok.erl\""
    ));
}

#[test]
fn a_binding_names_a_module_its_file_declares() {
    insta::assert_snapshot!(check_str(
        "extern fn read(path: String) -> String = \"files\":\"read\""
    ));
}

// ── C0066: use only as a tool handler ───────────────────────────

/// A call, a `let`, an argument, a lambda body, and a non-tool arm are all
/// rejected: the dispatcher would never see the call.
#[test]
fn an_extern_is_not_a_value() {
    insta::assert_snapshot!(check_str(&with_tool(
        "effect Log\n\
         extern fn erl_read_file(args: { path: String }) -> String = \"files\":\"read\"\n\
         fn apply(f: { path: String } -> String) -> String = f({ path: \"a\" })\n\
         fn call() -> String = erl_read_file({ path: \"a\" })\n\
         fn bind() -> Int = let f = erl_read_file in 1\n\
         fn pass() -> String = apply(erl_read_file)\n\
         fn wrap() -> String ! {Exn<Missing>} =\n\
           handle { Tool<ReadFile> -> \\a -> erl_read_file(a) } in read_file({ path: \"a\" })\n\
         fn bare() -> Int = handle { Log -> erl_read_file } in 1"
    )));
}

// ── C0067: the extern's row ─────────────────────────────────────

#[test]
fn an_extern_row_is_at_most_one_exn() {
    insta::assert_snapshot!(check_str(&with_tool(
        "type Other = Other(String)\n\
         extern fn tooly(args: { path: String }) -> String ! {Tool<ReadFile>} = \"files\":\"a\"\n\
         extern fn twice(args: { path: String }) -> String ! {Exn<Missing>, Exn<Other>}\n\
           = \"files\":\"b\"\n\
         extern fn open(args: { path: String }) -> String ! {Exn<Missing>, e} = \"files\":\"c\""
    )));
}

// ── C0068: the tool declares the extern's Exn ───────────────────

#[test]
fn the_tool_must_declare_the_extern_s_exn() {
    insta::assert_snapshot!(check_str(&with_tool(
        "type Other = Other(String)\n\
         tool Plain : { path: String } -> String\n\
         extern fn raises(args: { path: String }) -> String ! {Exn<Missing>} = \"files\":\"a\"\n\
         extern fn raises_other(args: { path: String }) -> String ! {Exn<Other>} = \"files\":\"b\"\n\
         fn main() -> () ! {Install} =\n\
           install {\n\
             Tool<Plain> -> raises,\n\
             Tool<ReadFile> -> raises_other,\n\
           } in ()"
    )));
}

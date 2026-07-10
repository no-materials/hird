// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Hirð-to-Erlang name mapping: modules, atoms, and variables.
//!
//! Three renamings cover the whole surface:
//!
//! - **Modules**: `hird_` plus the `snake_case` module path (`Planner` →
//!   `hird_planner`). The blanket prefix sidesteps collisions with OTP and
//!   stdlib module names instead of detecting them case by case.
//! - **Atoms** (function names, constructors, record labels, tool names):
//!   already `snake_case` in Hirð or snake-cased here; single-quoted whenever
//!   the spelling is not a valid unquoted Erlang atom or is a reserved word.
//! - **Variables**: Hirð `snake_case` binders capitalise their first letter
//!   (`foo_bar` → `Foo_bar`). Erlang variables cannot collide with reserved
//!   words (those are lowercase), and emitter-internal variables carry an `@`
//!   (`Handlers@`, `V@1`), which no Hirð identifier contains.

use alloc::format;
use alloc::string::String;

/// Erlang reserved words: not usable as unquoted atoms.
///
/// Includes `maybe`/`else` (reserved once the `maybe` expression is enabled,
/// the default in modern OTP) and the unused-but-reserved `cond`/`let`, so
/// output is safe across OTP releases.
const RESERVED: &[&str] = &[
    "after", "and", "andalso", "band", "begin", "bnot", "bor", "bsl", "bsr", "bxor", "case",
    "catch", "cond", "div", "else", "end", "fun", "if", "let", "maybe", "not", "of", "or",
    "orelse", "receive", "rem", "try", "when", "xor",
];

/// The Erlang module name of a Hirð module: `hird_` plus the snake-cased
/// path, with `.` separators joining as `_` (`Planner` → `hird_planner`).
#[must_use]
pub fn erlang_module_name(module: &str) -> String {
    let mut out = String::from("hird");
    for segment in module.split('.') {
        out.push('_');
        out.push_str(&snake_case(segment));
    }
    out
}

/// `PascalCase` to `snake_case`, with acronym runs kept whole (`ReadRepo` →
/// `read_repo`, `LLMCall` → `llm_call`). The same algorithm the checker uses
/// to derive a tool's generated function name, so tool atoms and tool
/// function names agree.
#[must_use]
pub(crate) fn snake_case(name: &str) -> String {
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

/// `name` as an Erlang atom: verbatim when it is a valid unquoted atom,
/// single-quoted (with `\` and `'` escaped) otherwise.
#[must_use]
pub(crate) fn atom(name: &str) -> String {
    if is_unquoted_atom(name) {
        return String::from(name);
    }
    let mut out = String::with_capacity(name.len() + 2);
    out.push('\'');
    for ch in name.chars() {
        if ch == '\'' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

/// Whether `name` is a valid unquoted Erlang atom: lowercase-led,
/// alphanumeric/`_`/`@` throughout, and not a reserved word.
fn is_unquoted_atom(name: &str) -> bool {
    let mut chars = name.chars();
    let leads = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    leads
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
        && !RESERVED.contains(&name)
}

/// The Erlang variable spelling of a Hirð binder: leading underscores kept,
/// first letter capitalised (`x` → `X`, `foo_bar` → `Foo_bar`, `_x` → `_X`).
/// An all-underscore or empty name maps to the anonymous `_` / a plain `V`.
#[must_use]
pub(crate) fn variable_base(name: &str) -> String {
    if name.is_empty() {
        return String::from("V");
    }
    if name.chars().all(|c| c == '_') {
        return String::from("_");
    }
    let mut out = String::with_capacity(name.len());
    let mut capitalised = false;
    for ch in name.chars() {
        if !capitalised && ch.is_ascii_alphabetic() {
            out.push(ch.to_ascii_uppercase());
            capitalised = true;
        } else {
            out.push(ch);
        }
    }
    if capitalised { out } else { format!("V{out}") }
}

#[cfg(test)]
mod tests {
    use super::{atom, erlang_module_name, snake_case, variable_base};

    #[test]
    fn module_names_are_prefixed_snake_case() {
        assert_eq!(erlang_module_name("Planner"), "hird_planner");
        assert_eq!(erlang_module_name("Repo.Utils"), "hird_repo_utils");
    }

    #[test]
    fn snake_case_keeps_acronym_runs() {
        assert_eq!(snake_case("ReadRepo"), "read_repo");
        assert_eq!(snake_case("LLMCall"), "llm_call");
        assert_eq!(snake_case("True"), "true");
    }

    #[test]
    fn reserved_and_invalid_atoms_are_quoted() {
        assert_eq!(atom("read_repo"), "read_repo");
        assert_eq!(atom("div"), "'div'");
        assert_eq!(atom("end"), "'end'");
        assert_eq!(atom("Weird"), "'Weird'");
    }

    #[test]
    fn variables_capitalise_and_keep_underscores() {
        assert_eq!(variable_base("x"), "X");
        assert_eq!(variable_base("foo_bar"), "Foo_bar");
        assert_eq!(variable_base("_hint"), "_Hint");
        assert_eq!(variable_base("_"), "_");
        assert_eq!(variable_base(""), "V");
    }
}

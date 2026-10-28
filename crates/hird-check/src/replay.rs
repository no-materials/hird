// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Log-file replay: loading a recorded audit log and driving
//! strict-sequential replay across a whole run, one stream per process.
//!
//! The replay core ([`wire::replay`](replay())) is pure and per-record.
//! This module is the layer that turns it into a run driver: [`ToolTable`]
//! collects the wire signatures of a checked program's tools, [`load_log`]
//! parses a JSON-lines log type-directedly against them, and
//! [`ReplayCursor`] matches each process's tool calls against that
//! process's records in order — the process a record's caller id names
//! ([`process_of`]) — returning each logged result. Any mismatch surfaces
//! as a [`DivergenceReport`]: the core's structural [`Divergence`]
//! enriched with the offering process, the log's extent, the expected
//! (logged) record with its caller, and the offered call — rendered
//! actionably by its `Display`.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::CheckedFile;
use crate::wire::{
    AdtTable, DecodeError, Divergence, InvocationRecord, ToolResult, ToolWireSig, WireValue,
    decode_record, encode_value, peek_tool, replay,
};

// ── loading ─────────────────────────────────────────────────────

/// The wire signatures a log decodes against, keyed by declared tool name
/// (`ReadRepo`, as recorded in the envelope's `tool` field).
#[derive(Debug, Default)]
pub struct ToolTable {
    /// Tool name to its wire signature.
    entries: BTreeMap<String, ToolWireSig>,
}

impl ToolTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a tool's wire signature.
    pub fn insert(&mut self, tool: impl Into<String>, sig: ToolWireSig) {
        self.entries.insert(tool.into(), sig);
    }

    /// The signatures of every tool a checked file declares.
    #[must_use]
    pub fn from_checked(checked: &CheckedFile) -> Self {
        let entries = checked
            .tools
            .iter()
            .filter_map(|(name, scheme)| {
                ToolWireSig::from_fn(scheme).map(|sig| (String::from(name.as_str()), sig))
            })
            .collect();
        Self { entries }
    }

    /// The signature of `tool`, if declared.
    #[must_use]
    pub fn get(&self, tool: &str) -> Option<&ToolWireSig> {
        self.entries.get(tool)
    }
}

/// Why a log file failed to load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    /// 1-based line number of the offending line.
    pub line: usize,
    /// What went wrong on that line.
    pub kind: LoadErrorKind,
}

/// What went wrong on one log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadErrorKind {
    /// The line records a tool the program does not declare.
    UnknownTool(String),
    /// The line failed to decode against the tool's signature.
    Decode(DecodeError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            LoadErrorKind::UnknownTool(tool) => write!(
                f,
                "line {}: the log records tool `{tool}`, which the program does not declare",
                self.line
            ),
            LoadErrorKind::Decode(error) => write!(f, "line {}: {error}", self.line),
        }
    }
}

/// Parses and decodes a JSON-lines audit log against `tools`: the envelope
/// names each record's tool, whose signature directs the value decoding.
/// Every line must decode; there is no skipping.
///
/// # Errors
///
/// A [`LoadError`] with a 1-based line number when a line names an
/// undeclared tool or fails to decode.
pub fn load_log(
    input: &str,
    tools: &ToolTable,
    adts: &AdtTable,
) -> Result<Vec<InvocationRecord>, LoadError> {
    let mut records = Vec::new();
    for (i, line) in input.lines().enumerate() {
        let fail = |kind| LoadError { line: i + 1, kind };
        let tool = peek_tool(line).map_err(|e| fail(LoadErrorKind::Decode(e)))?;
        let Some(sig) = tools.get(&tool) else {
            return Err(fail(LoadErrorKind::UnknownTool(tool)));
        };
        let record = decode_record(line, sig, adts).map_err(|e| fail(LoadErrorKind::Decode(e)))?;
        records.push(record);
    }
    Ok(records)
}

// ── the cursor ──────────────────────────────────────────────────

/// The process a caller id names after `@` (`"Worker.init@Sup/worker"`
/// names `Sup/worker`); `None` when it names none, which places the record
/// in the anonymous stream.
#[must_use]
pub fn process_of(caller: &str) -> Option<&str> {
    caller.split_once('@').map(|(_, process)| process)
}

/// A cursor driving strict-sequential replay across a decoded log, one
/// stream per process.
///
/// Each [`offer`](Self::offer) matches a process's next tool call against
/// the next record of that process's stream: a match returns the logged
/// result and advances the stream by one; a divergence reports without
/// advancing. Streams interleave freely. A green replay ends with
/// [`remaining`](Self::remaining) at zero.
#[derive(Debug)]
pub struct ReplayCursor<'log> {
    /// The decoded run.
    log: &'log [InvocationRecord],
    /// Each named process's unconsumed records, as log positions in order.
    named: BTreeMap<&'log str, VecDeque<usize>>,
    /// The unconsumed records whose caller names no process.
    anonymous: VecDeque<usize>,
    /// Records consumed so far, across every stream.
    consumed: usize,
}

impl<'log> ReplayCursor<'log> {
    /// A cursor at the start of every stream of `log`.
    #[must_use]
    pub fn new(log: &'log [InvocationRecord]) -> Self {
        let mut named: BTreeMap<&'log str, VecDeque<usize>> = BTreeMap::new();
        let mut anonymous = VecDeque::new();
        for (position, record) in log.iter().enumerate() {
            match process_of(&record.caller) {
                Some(process) => named.entry(process).or_default().push_back(position),
                None => anonymous.push_back(position),
            }
        }
        Self {
            log,
            named,
            anonymous,
            consumed: 0,
        }
    }

    /// Records consumed so far, across every stream.
    #[must_use]
    pub fn consumed(&self) -> usize {
        self.consumed
    }

    /// Records not yet consumed.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.log.len() - self.consumed
    }

    /// Offers `process`'s next tool call (`None` for a process with no
    /// identity, which offers against the anonymous stream).
    ///
    /// # Errors
    ///
    /// A [`DivergenceReport`] when the call does not match the next record
    /// of the process's stream; the stream does not advance.
    pub fn offer(
        &mut self,
        process: Option<&str>,
        tool: &str,
        args: &WireValue,
    ) -> Result<&'log ToolResult, Box<DivergenceReport>> {
        let stream = match process {
            Some(process) => self.named.get_mut(process),
            None => Some(&mut self.anonymous),
        };
        // An exhausted stream offers past the end of the log.
        let position = stream
            .as_ref()
            .and_then(|stream| stream.front().copied())
            .unwrap_or(self.log.len());
        match replay(self.log, position, tool, args) {
            Ok(result) => {
                if let Some(stream) = stream {
                    stream.pop_front();
                }
                self.consumed += 1;
                Ok(result)
            }
            Err(divergence) => Err(Box::new(DivergenceReport {
                divergence,
                process: process.map(String::from),
                expected: self.log.get(position).cloned(),
                offered_tool: String::from(tool),
                offered_args: args.clone(),
                log_len: self.log.len(),
            })),
        }
    }
}

// ── divergence reporting ────────────────────────────────────────

/// A [`Divergence`] enriched with everything a diagnosis needs: the
/// offering process, the expected (logged) record with its caller, the
/// offered call, and the log's extent. `Display` renders it as an
/// actionable multi-line message.
#[derive(Debug, Clone, PartialEq)]
pub struct DivergenceReport {
    /// The structural divergence from the replay core; its position is the
    /// log position of the record the process's stream expected, or the
    /// log's length when that stream is exhausted.
    pub divergence: Divergence,
    /// The process whose call diverged; `None` for the anonymous stream.
    pub process: Option<String>,
    /// The logged record the run expected at the divergence position;
    /// `None` when the process's stream is exhausted.
    pub expected: Option<InvocationRecord>,
    /// The tool the program offered.
    pub offered_tool: String,
    /// The args the program offered.
    pub offered_args: WireValue,
    /// Total records in the log.
    pub log_len: usize,
}

impl DivergenceReport {
    /// Writes the opening through its colon: the position, the log's
    /// extent, and the process when it is named.
    fn opening(&self, f: &mut fmt::Formatter<'_>, position: usize) -> fmt::Result {
        write!(
            f,
            "replay diverged at position {position} of {}",
            self.log_len
        )?;
        if let Some(process) = &self.process {
            write!(f, " in process `{process}`")?;
        }
        f.write_str(": ")
    }

    /// Writes the `expected:` line — the logged record's call and caller.
    fn expected_line(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.expected {
            Some(record) => writeln!(
                f,
                "  expected: {} {} (caller {})",
                record.tool,
                render_args(&record.args),
                record.caller
            ),
            None => Ok(()),
        }
    }

    /// Writes the `offered:` line — the call the program made.
    fn offered_line(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  offered:  {} {}",
            self.offered_tool,
            render_args(&self.offered_args)
        )
    }
}

impl fmt::Display for DivergenceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.divergence {
            Divergence::Exhausted { position, .. } => {
                self.opening(f, *position)?;
                if self.process.is_some() {
                    writeln!(f, "the log holds no further calls for this process")?;
                } else {
                    writeln!(f, "the log is exhausted")?;
                }
            }
            Divergence::ToolMismatch {
                position,
                logged,
                requested,
            } => {
                self.opening(f, *position)?;
                writeln!(
                    f,
                    "the log expects `{logged}`, the program offered `{requested}`"
                )?;
                self.expected_line(f)?;
            }
            Divergence::ArgsMismatch { position, .. } => {
                self.opening(f, *position)?;
                writeln!(f, "`{}` matches but the args differ", self.offered_tool)?;
                self.expected_line(f)?;
            }
        }
        self.offered_line(f)
    }
}

/// A call's args in canonical wire encoding (debug form for the
/// unencodable).
fn render_args(args: &WireValue) -> String {
    encode_value(args).unwrap_or_else(|_| format!("{args:?}"))
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;

    use hird_types::{Label, Type};

    use super::*;
    use crate::wire::encode_record;

    /// The `Ping : { n: Int } → Int` wire signature.
    fn ping_sig() -> ToolWireSig {
        ToolWireSig {
            args: Type::record([(Label::new("n"), Type::int())]),
            result: Type::int(),
            errors: vec![],
        }
    }

    /// A table declaring only `Ping`.
    fn ping_table() -> ToolTable {
        let mut table = ToolTable::new();
        table.insert("Ping", ping_sig());
        table
    }

    /// A recorded invocation of `tool` with args and result `n`.
    fn ping_record(tool: &str, n: i64) -> InvocationRecord {
        InvocationRecord {
            tool: String::from(tool),
            args: WireValue::record([("n", WireValue::Int(n))]),
            result: ToolResult::Ok(WireValue::Int(n)),
            timestamp: String::from("2026-05-22T12:00:00.000Z"),
            caller: String::from("M.f"),
            meta: None,
        }
    }

    /// `records` encoded as a JSON-lines log.
    fn encode_log(records: &[InvocationRecord]) -> String {
        let mut out = String::new();
        for record in records {
            out.push_str(&encode_record(record).unwrap());
            out.push('\n');
        }
        out
    }

    #[test]
    fn load_log_decodes_every_line() {
        let log = encode_log(&[ping_record("Ping", 1), ping_record("Ping", 2)]);
        let records = load_log(&log, &ping_table(), &AdtTable::new()).unwrap();
        assert_eq!(
            records,
            vec![ping_record("Ping", 1), ping_record("Ping", 2)]
        );
        assert_eq!(
            load_log("", &ping_table(), &AdtTable::new()).unwrap(),
            vec![],
            "an empty log loads empty"
        );
    }

    #[test]
    fn an_undeclared_tool_fails_with_its_line_number() {
        let log = encode_log(&[ping_record("Ping", 1), ping_record("Pong", 2)]);
        let err = load_log(&log, &ping_table(), &AdtTable::new()).unwrap_err();
        assert_eq!(err.line, 2);
        assert_eq!(err.kind, LoadErrorKind::UnknownTool(String::from("Pong")));
        assert_eq!(
            err.to_string(),
            "line 2: the log records tool `Pong`, which the program does not declare"
        );
    }

    #[test]
    fn a_malformed_line_fails_with_its_line_number() {
        let mut log = encode_log(&[ping_record("Ping", 1)]);
        log.push_str("not json\n");
        let err = load_log(&log, &ping_table(), &AdtTable::new()).unwrap_err();
        assert_eq!(err.line, 2);
        assert!(matches!(err.kind, LoadErrorKind::Decode(_)), "{err}");
        assert!(err.to_string().starts_with("line 2: decode error"), "{err}");
    }

    #[test]
    fn the_cursor_advances_only_on_matches() {
        let log = vec![ping_record("Ping", 1), ping_record("Ping", 2)];
        let mut cursor = ReplayCursor::new(&log);
        assert_eq!((cursor.consumed(), cursor.remaining()), (0, 2));
        let report = cursor.offer(None, "Pong", &log[0].args).unwrap_err();
        assert!(matches!(report.divergence, Divergence::ToolMismatch { .. }));
        assert_eq!(cursor.consumed(), 0, "a divergence must not advance");
        let result = cursor.offer(None, "Ping", &log[0].args).unwrap();
        assert_eq!(result, &log[0].result);
        assert_eq!((cursor.consumed(), cursor.remaining()), (1, 1));
    }

    /// `ping_record("Ping", n)` logged by the process `process`.
    fn ping_by(process: &str, n: i64) -> InvocationRecord {
        InvocationRecord {
            caller: format!("M.f@{process}"),
            ..ping_record("Ping", n)
        }
    }

    #[test]
    fn a_caller_names_its_process_after_the_at_sign() {
        assert_eq!(process_of("Worker.init@Sup/worker"), Some("Sup/worker"));
        assert_eq!(
            process_of("Worker.handle_msg/Go@main/Worker#2"),
            Some("main/Worker#2")
        );
        assert_eq!(process_of("M.f"), None);
    }

    #[test]
    fn each_process_replays_its_own_stream_in_any_interleaving() {
        let log = vec![ping_by("a", 1), ping_by("b", 2), ping_by("a", 3)];
        let mut cursor = ReplayCursor::new(&log);
        let report = cursor.offer(Some("a"), "Ping", &log[2].args).unwrap_err();
        assert!(
            matches!(
                report.divergence,
                Divergence::ArgsMismatch { position: 0, .. }
            ),
            "{report}"
        );
        assert_eq!(report.process.as_deref(), Some("a"));
        for record in [&log[1], &log[0], &log[2]] {
            let result = cursor
                .offer(process_of(&record.caller), "Ping", &record.args)
                .unwrap();
            assert_eq!(result, &record.result);
        }
        assert_eq!(cursor.remaining(), 0);
        let report = cursor.offer(Some("b"), "Ping", &log[1].args).unwrap_err();
        assert!(
            matches!(report.divergence, Divergence::Exhausted { position: 3, .. }),
            "{report}"
        );
        assert_eq!(
            report.to_string(),
            "replay diverged at position 3 of 3 in process `b`: \
             the log holds no further calls for this process\n  \
             offered:  Ping {\"n\":2}"
        );
    }

    #[test]
    fn a_process_never_matches_another_stream() {
        let log = vec![ping_record("Ping", 1), ping_by("a", 2)];
        let mut cursor = ReplayCursor::new(&log);
        let report = cursor.offer(Some("b"), "Ping", &log[1].args).unwrap_err();
        assert!(matches!(report.divergence, Divergence::Exhausted { .. }));
        let report = cursor.offer(None, "Ping", &log[1].args).unwrap_err();
        assert!(
            matches!(
                report.divergence,
                Divergence::ArgsMismatch { position: 0, .. }
            ),
            "the anonymous stream holds only the record naming no process"
        );
    }
}

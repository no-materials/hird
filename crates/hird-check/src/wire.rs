// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The tool-invocation wire format: canonical JSON encoding of invocation
//! records, the audit sink, and strict-sequential replay.
//!
//! This is the reference implementation of the audit-log format specified
//! normatively in `docs/tool-effects.md`. Every tool invocation is one
//! JSON-lines record with envelope fields in fixed order —
//! `schema_version`, `tool`, `args`, `result`, `timestamp`, `caller`, and
//! an optional observer-populated `meta`. The writer is canonical: for a
//! given record it produces exactly one byte sequence (no whitespace,
//! sorted record labels, shortest round-trip floats), so logs are diffable
//! and a future runtime can be conformance-tested against golden files
//! byte for byte.
//!
//! Values encode type-directedly and injectively per type: ADT values as
//! `{"ctor":…,"args":…}`, unit as `null`, records as label-sorted objects,
//! lists and tuples as arrays. Decoding therefore validates against the
//! tool's signature — [`decode_value`] takes the expected [`Type`] and an
//! [`AdtTable`] of constructor shapes.
//!
//! Replay is a pure function over a decoded log: [`replay`] matches
//! records strictly in order and returns the logged result, failing with a
//! structured [`Divergence`] on any mismatch. Whether a program replays or
//! re-executes is a handler decision, not a language mode.

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use hird_types::{Label, Name, Type};

use crate::CheckedFile;

/// The wire-format version this implementation writes and accepts.
pub const SCHEMA_VERSION: u32 = 1;

// ── values ──────────────────────────────────────────────────────

/// A Hirð value as it crosses the tool wire boundary.
///
/// Function types and capabilities have no variant: the checker rejects
/// them in tool signatures, so they are unrepresentable here by
/// construction. Floats must be finite; NaN and the infinities are not
/// wire-representable and fail at encode time.
#[derive(Debug, Clone, PartialEq)]
pub enum WireValue {
    /// The unit value, encoded as `null`.
    Unit,
    /// An integer, exact within `i64`.
    Int(i64),
    /// A finite float, encoded shortest-round-trip.
    Float(f64),
    /// A string.
    String(String),
    /// A list, encoded as a JSON array.
    List(Vec<Self>),
    /// A tuple, encoded as a JSON array of its fixed arity. Never empty —
    /// the empty tuple is [`WireValue::Unit`].
    Tuple(Vec<Self>),
    /// A structural record, encoded as an object with label-sorted keys.
    Record(BTreeMap<Label, Self>),
    /// An ADT value, encoded as `{"ctor":"Name","args":[…]}`. `Bool` is an
    /// ADT like any other: `True` and `False` are nullary constructors.
    Ctor(Name, Vec<Self>),
}

impl WireValue {
    /// A string value.
    #[must_use]
    pub fn string(s: impl Into<String>) -> Self {
        Self::String(s.into())
    }

    /// A record from `(label, value)` pairs.
    #[must_use]
    pub fn record(fields: impl IntoIterator<Item = (&'static str, Self)>) -> Self {
        Self::Record(
            fields
                .into_iter()
                .map(|(label, value)| (Label::new(label), value))
                .collect(),
        )
    }

    /// A constructor application.
    #[must_use]
    pub fn ctor(name: impl Into<Name>, args: Vec<Self>) -> Self {
        Self::Ctor(name.into(), args)
    }
}

/// A tool invocation's outcome: failures are first-class on the wire, so a
/// log replays errors as faithfully as successes.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolResult {
    /// The tool returned a value, encoded as `{"ok":…}`.
    Ok(WireValue),
    /// The tool failed with an error value, encoded as `{"err":…}`.
    Err(WireValue),
}

/// An entry of the observer-populated `meta` envelope field: plain JSON,
/// self-describing rather than signature-typed, because transport metadata
/// (`duration_ms`, retries, trace ids) is no part of the compiler-derived
/// invocation record.
#[derive(Debug, Clone, PartialEq)]
pub enum MetaValue {
    /// JSON `null`.
    Null,
    /// A JSON boolean.
    Bool(bool),
    /// A JSON number with integer syntax.
    Int(i64),
    /// A JSON number with fractional or exponent syntax.
    Float(f64),
    /// A JSON string.
    String(String),
    /// A JSON array.
    Array(Vec<Self>),
    /// A JSON object, keys sorted.
    Object(BTreeMap<String, Self>),
}

/// One tool invocation as recorded in the audit log.
///
/// `schema_version` is not a field: the writer stamps [`SCHEMA_VERSION`]
/// on every record and the decoder accepts only that version. `timestamp`
/// and `caller` are injected by the recording handler — there is no
/// ambient clock. The caller is `"Module.function"`, or the actor form
/// (`"Planner.handle_msg/PlanRepo"`) inside generated actor callbacks;
/// decoders treat it as an opaque string.
#[derive(Debug, Clone, PartialEq)]
pub struct InvocationRecord {
    /// The tool's declared name (e.g. `ReadRepo`).
    pub tool: String,
    /// The structured arguments the tool was invoked with.
    pub args: WireValue,
    /// The tagged outcome.
    pub result: ToolResult,
    /// RFC 3339 UTC instant with millisecond precision
    /// (`2026-05-22T12:00:00.000Z`).
    pub timestamp: String,
    /// The invoking function, as `"Module.function"`.
    pub caller: String,
    /// Observer-populated transport metadata; omitted from the wire when
    /// `None`.
    pub meta: Option<BTreeMap<String, MetaValue>>,
}

// ── encoding ────────────────────────────────────────────────────

/// Why a value or record could not be encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// A float was NaN or infinite.
    NonFiniteFloat,
    /// A timestamp was not RFC 3339 UTC with millisecond precision.
    BadTimestamp(String),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteFloat => f.write_str("NaN and infinities are not wire-representable"),
            Self::BadTimestamp(ts) => {
                write!(
                    f,
                    "timestamp `{ts}` is not of the form 2026-05-22T12:00:00.000Z"
                )
            }
        }
    }
}

/// Encodes a value in canonical form: no whitespace, label-sorted record
/// keys, shortest round-trip floats.
pub fn encode_value(value: &WireValue) -> Result<String, EncodeError> {
    let mut out = String::new();
    write_value(&mut out, value)?;
    Ok(out)
}

/// Encodes a record as one canonical JSON line (no trailing newline), with
/// the envelope fields in fixed order.
pub fn encode_record(record: &InvocationRecord) -> Result<String, EncodeError> {
    if !is_valid_timestamp(&record.timestamp) {
        return Err(EncodeError::BadTimestamp(record.timestamp.clone()));
    }
    let mut out = String::new();
    out.push_str("{\"schema_version\":");
    out.push_str(&format!("{SCHEMA_VERSION}"));
    out.push_str(",\"tool\":");
    write_string(&mut out, &record.tool);
    out.push_str(",\"args\":");
    write_value(&mut out, &record.args)?;
    out.push_str(",\"result\":");
    match &record.result {
        ToolResult::Ok(value) => {
            out.push_str("{\"ok\":");
            write_value(&mut out, value)?;
        }
        ToolResult::Err(value) => {
            out.push_str("{\"err\":");
            write_value(&mut out, value)?;
        }
    }
    out.push_str("},\"timestamp\":");
    write_string(&mut out, &record.timestamp);
    out.push_str(",\"caller\":");
    write_string(&mut out, &record.caller);
    if let Some(meta) = &record.meta {
        out.push_str(",\"meta\":");
        write_meta(&mut out, meta)?;
    }
    out.push('}');
    Ok(out)
}

/// Writes `value` in canonical form onto `out`.
fn write_value(out: &mut String, value: &WireValue) -> Result<(), EncodeError> {
    match value {
        WireValue::Unit => out.push_str("null"),
        WireValue::Int(i) => out.push_str(&format!("{i}")),
        WireValue::Float(f) => write_float(out, *f)?,
        WireValue::String(s) => write_string(out, s),
        WireValue::List(items) | WireValue::Tuple(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item)?;
            }
            out.push(']');
        }
        WireValue::Record(fields) => {
            out.push('{');
            for (i, (label, field)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, label.as_str());
                out.push(':');
                write_value(out, field)?;
            }
            out.push('}');
        }
        WireValue::Ctor(name, args) => {
            out.push_str("{\"ctor\":");
            write_string(out, name.as_str());
            out.push_str(",\"args\":[");
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, arg)?;
            }
            out.push_str("]}");
        }
    }
    Ok(())
}

/// Writes a finite float in its shortest round-trip decimal form (plain
/// notation, no exponent; integral values print without a fraction).
fn write_float(out: &mut String, value: f64) -> Result<(), EncodeError> {
    if !value.is_finite() {
        return Err(EncodeError::NonFiniteFloat);
    }
    out.push_str(&format!("{value}"));
    Ok(())
}

/// Writes a JSON string: `"` and `\` escaped, control characters as their
/// short escapes or `\u00XX`, everything else (including non-ASCII) verbatim.
fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Writes a `meta` object canonically: keys sorted, no whitespace.
fn write_meta(out: &mut String, meta: &BTreeMap<String, MetaValue>) -> Result<(), EncodeError> {
    out.push('{');
    for (i, (key, value)) in meta.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_string(out, key);
        out.push(':');
        write_meta_value(out, value)?;
    }
    out.push('}');
    Ok(())
}

/// Writes one `meta` value.
fn write_meta_value(out: &mut String, value: &MetaValue) -> Result<(), EncodeError> {
    match value {
        MetaValue::Null => out.push_str("null"),
        MetaValue::Bool(true) => out.push_str("true"),
        MetaValue::Bool(false) => out.push_str("false"),
        MetaValue::Int(i) => out.push_str(&format!("{i}")),
        MetaValue::Float(f) => write_float(out, *f)?,
        MetaValue::String(s) => write_string(out, s),
        MetaValue::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_meta_value(out, item)?;
            }
            out.push(']');
        }
        MetaValue::Object(fields) => write_meta(out, fields)?,
    }
    Ok(())
}

/// Whether `s` is an RFC 3339 UTC instant with millisecond precision:
/// `YYYY-MM-DDTHH:MM:SS.mmmZ`, with in-range fields (days are checked
/// against 31, not the month's calendar length).
fn is_valid_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 24 {
        return false;
    }
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21, 22];
    if !digits.iter().all(|&i| b[i].is_ascii_digit()) {
        return false;
    }
    if b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return false;
    }
    if b[19] != b'.' || b[23] != b'Z' {
        return false;
    }
    let field = |from: usize, to: usize| -> u32 { s[from..to].parse().unwrap_or(u32::MAX) };
    (1..=12).contains(&field(5, 7))
        && (1..=31).contains(&field(8, 10))
        && field(11, 13) < 24
        && field(14, 16) < 60
        && field(17, 19) < 60
}

// ── the audit sink ──────────────────────────────────────────────

/// Where invocation records go. The sink is a capability: a recording
/// handler receives one explicitly, and emission is visible in the effect
/// row — never implicit in tool dispatch.
pub trait AuditSink {
    /// Records one invocation.
    ///
    /// # Errors
    ///
    /// If the record cannot be encoded (non-finite float, bad timestamp).
    fn emit(&mut self, record: &InvocationRecord) -> Result<(), EncodeError>;
}

/// The default sink: canonical JSON lines appended to an in-memory buffer,
/// one record per line.
#[derive(Debug, Default)]
pub struct JsonLinesSink {
    /// The accumulated log.
    buf: String,
}

impl JsonLinesSink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The log so far: one JSON record per `\n`-terminated line.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.buf
    }

    /// Consumes the sink, returning the log.
    #[must_use]
    pub fn into_string(self) -> String {
        self.buf
    }
}

impl AuditSink for JsonLinesSink {
    fn emit(&mut self, record: &InvocationRecord) -> Result<(), EncodeError> {
        self.buf.push_str(&encode_record(record)?);
        self.buf.push('\n');
        Ok(())
    }
}

// ── decoding ────────────────────────────────────────────────────

/// Why a wire string failed to decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    /// Byte offset of the failure in the input.
    pub offset: usize,
    /// What went wrong.
    pub message: String,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "decode error at byte {}: {}", self.offset, self.message)
    }
}

/// The constructor shapes decoding validates ADT values against: each type
/// name maps to its constructors, each with field types where `TyVar(i)`
/// stands for the ADT's `i`-th type parameter.
#[derive(Debug, Default)]
pub struct AdtTable {
    /// Type name to `(constructor, field types)` in declaration order.
    entries: BTreeMap<Name, Vec<(Name, Vec<Type>)>>,
}

impl AdtTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an ADT's constructors.
    pub fn insert(&mut self, adt: Name, ctors: Vec<(Name, Vec<Type>)>) {
        self.entries.insert(adt, ctors);
    }

    /// The table of every ADT a checked file declares, with constructor
    /// field types recovered from the constructors' bound schemes.
    #[must_use]
    pub fn from_checked(checked: &CheckedFile) -> Self {
        let mut table = Self::new();
        for (adt, ctor_names) in &checked.adts {
            let mut ctors = Vec::new();
            for ctor in ctor_names {
                let Some(scheme) = checked.bindings.get(ctor.as_str()) else {
                    continue;
                };
                ctors.push((ctor.clone(), scheme_fields(scheme)));
            }
            table.insert(adt.clone(), ctors);
        }
        table
    }

    /// The constructors of `adt`, if registered.
    fn ctors(&self, adt: &Name) -> Option<&[(Name, Vec<Type>)]> {
        self.entries.get(adt).map(Vec::as_slice)
    }
}

/// A constructor scheme's field types, with the owning ADT's type
/// parameters renumbered positionally: `∀a. a → Option<a>` yields
/// `[TyVar(0)]` regardless of the quantified variable's id.
fn scheme_fields(scheme: &Type) -> Vec<Type> {
    let body = match scheme {
        Type::TyForall(_, _, body) => body.as_ref(),
        other => other,
    };
    let Type::TyFn(fields, ret, _) = body else {
        // Nullary constructors bind the bare instance type.
        return Vec::new();
    };
    let mut positions = BTreeMap::new();
    if let Type::TyCon(_, ret_args) = ret.as_ref() {
        for (i, arg) in ret_args.iter().enumerate() {
            if let Type::TyVar(v) = arg {
                positions.insert(*v, Type::TyVar(u32::try_from(i).unwrap_or(u32::MAX)));
            }
        }
    }
    let rows = BTreeMap::new();
    fields
        .iter()
        .map(|f| f.substitute(&positions, &rows))
        .collect()
}

/// The signature a tool's records decode against.
#[derive(Debug, Clone)]
pub struct ToolWireSig {
    /// The args type (the tool's input record).
    pub args: Type,
    /// The ok-result type (the tool's output).
    pub result: Type,
    /// The error types the tool's row declares (`Exn<E>` arguments), tried
    /// in order when decoding an `err` result.
    pub errors: Vec<Type>,
}

impl ToolWireSig {
    /// Extracts the signature from a tool function's bound type
    /// (`(args) → result ! {Tool<Name>, Exn<E>, …}`). `None` when the type
    /// is not a unary function.
    #[must_use]
    pub fn from_fn(ty: &Type) -> Option<Self> {
        let body = match ty {
            Type::TyForall(_, _, body) => body.as_ref(),
            other => other,
        };
        let Type::TyFn(params, ret, row) = body else {
            return None;
        };
        let [args] = params.as_slice() else {
            return None;
        };
        let errors = row
            .effects()
            .filter(|e| e.head().as_str() == "Exn")
            .filter_map(|e| e.args().first().cloned())
            .collect();
        Some(Self {
            args: args.clone(),
            result: (**ret).clone(),
            errors,
        })
    }
}

/// Decodes a value against its expected type, validating shape, labels,
/// constructor names, and arities as it goes.
pub fn decode_value(input: &str, ty: &Type, adts: &AdtTable) -> Result<WireValue, DecodeError> {
    let mut reader = Reader::new(input);
    reader.skip_ws();
    let value = reader.value(ty, adts)?;
    reader.skip_ws();
    reader.end()?;
    Ok(value)
}

/// Decodes one audit-log line against a tool's signature. The envelope
/// fields must appear in the canonical fixed order, and `schema_version`
/// must be [`SCHEMA_VERSION`].
pub fn decode_record(
    input: &str,
    sig: &ToolWireSig,
    adts: &AdtTable,
) -> Result<InvocationRecord, DecodeError> {
    let mut reader = Reader::new(input);
    let tool = reader.envelope_tool()?;
    reader.expect(b',')?;
    reader.key("args")?;
    let args = reader.value(&sig.args, adts)?;
    reader.expect(b',')?;
    reader.key("result")?;
    let result = reader.result(sig, adts)?;
    reader.expect(b',')?;
    reader.key("timestamp")?;
    let timestamp = reader.string()?;
    if !is_valid_timestamp(&timestamp) {
        return Err(reader.error(format!(
            "timestamp `{timestamp}` is not of the form 2026-05-22T12:00:00.000Z"
        )));
    }
    reader.expect(b',')?;
    reader.key("caller")?;
    let caller = reader.string()?;
    let meta = if reader.peek() == Some(b',') {
        reader.expect(b',')?;
        reader.key("meta")?;
        Some(reader.meta_object()?)
    } else {
        None
    };
    reader.expect(b'}')?;
    reader.skip_ws();
    reader.end()?;
    Ok(InvocationRecord {
        tool,
        args,
        result,
        timestamp,
        caller,
        meta,
    })
}

/// Reads the tool name from a record line's envelope prefix without
/// decoding the rest — enough to select the [`ToolWireSig`] a full
/// [`decode_record`] needs.
///
/// # Errors
///
/// If the envelope prefix is malformed or the `schema_version` is not
/// [`SCHEMA_VERSION`].
pub fn peek_tool(input: &str) -> Result<String, DecodeError> {
    Reader::new(input).envelope_tool()
}

/// A cursor over the bytes of one wire string.
struct Reader<'a> {
    /// The input.
    input: &'a str,
    /// Current byte offset.
    pos: usize,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `input`.
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    /// A decode error at the current offset.
    fn error(&self, message: String) -> DecodeError {
        DecodeError {
            offset: self.pos,
            message,
        }
    }

    /// The byte at the cursor, if any.
    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    /// Skips JSON whitespace.
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// Consumes exactly `byte` (after whitespace).
    fn expect(&mut self, byte: u8) -> Result<(), DecodeError> {
        self.skip_ws();
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(format!("expected `{}`", byte as char)))
        }
    }

    /// Fails unless the input is exhausted.
    fn end(&self) -> Result<(), DecodeError> {
        if self.pos == self.input.len() {
            Ok(())
        } else {
            Err(self.error(String::from("trailing input after the record")))
        }
    }

    /// Consumes the envelope prefix through the tool name: `{`, a
    /// `schema_version` checked against [`SCHEMA_VERSION`], and the `tool`
    /// key with its string value.
    fn envelope_tool(&mut self) -> Result<String, DecodeError> {
        self.skip_ws();
        self.expect(b'{')?;
        self.key("schema_version")?;
        let version = self.integer()?;
        if version != i64::from(SCHEMA_VERSION) {
            return Err(self.error(format!(
                "unsupported schema_version {version}, expected {SCHEMA_VERSION}"
            )));
        }
        self.expect(b',')?;
        self.key("tool")?;
        self.string()
    }

    /// Consumes the object key `name` and its `:`.
    fn key(&mut self, name: &str) -> Result<(), DecodeError> {
        let key = self.string()?;
        if key != name {
            return Err(self.error(format!("expected key `{name}`, found `{key}`")));
        }
        self.expect(b':')
    }

    /// Consumes the literal `lit`.
    fn literal(&mut self, lit: &str) -> Result<(), DecodeError> {
        if self.input[self.pos..].starts_with(lit) {
            self.pos += lit.len();
            Ok(())
        } else {
            Err(self.error(format!("expected `{lit}`")))
        }
    }

    /// Parses a JSON string.
    fn string(&mut self) -> Result<String, DecodeError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let rest = &self.input[self.pos..];
            let Some(c) = rest.chars().next() else {
                return Err(self.error(String::from("unterminated string")));
            };
            self.pos += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => out.push(self.escape()?),
                c if (c as u32) < 0x20 => {
                    return Err(self.error(String::from("raw control character in string")));
                }
                c => out.push(c),
            }
        }
    }

    /// Parses one string escape, the leading `\` already consumed.
    fn escape(&mut self) -> Result<char, DecodeError> {
        let Some(b) = self.peek() else {
            return Err(self.error(String::from("unterminated escape")));
        };
        self.pos += 1;
        Ok(match b {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode_escape(),
            other => {
                return Err(self.error(format!("unknown escape `\\{}`", other as char)));
            }
        })
    }

    /// Parses `XXXX` of a `\uXXXX` escape, pairing surrogates.
    fn unicode_escape(&mut self) -> Result<char, DecodeError> {
        let high = self.hex4()?;
        if (0xD800..0xDC00).contains(&high) {
            self.literal("\\u")?;
            let low = self.hex4()?;
            if !(0xDC00..0xE000).contains(&low) {
                return Err(self.error(String::from("unpaired surrogate escape")));
            }
            let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
            return char::from_u32(code)
                .ok_or_else(|| self.error(String::from("invalid surrogate pair")));
        }
        char::from_u32(high).ok_or_else(|| self.error(String::from("invalid unicode escape")))
    }

    /// Parses four hex digits.
    fn hex4(&mut self) -> Result<u32, DecodeError> {
        let Some(hex) = self.input.get(self.pos..self.pos + 4) else {
            return Err(self.error(String::from("truncated unicode escape")));
        };
        let code = u32::from_str_radix(hex, 16)
            .map_err(|_| self.error(String::from("invalid unicode escape")))?;
        self.pos += 4;
        Ok(code)
    }

    /// The span of the JSON number at the cursor.
    fn number_str(&mut self) -> Result<&'a str, DecodeError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(self.error(String::from("expected a number")));
        }
        Ok(&self.input[start..self.pos])
    }

    /// Parses an integer-syntax number as `i64`.
    fn integer(&mut self) -> Result<i64, DecodeError> {
        self.skip_ws();
        let text = self.number_str()?;
        text.parse()
            .map_err(|_| self.error(format!("`{text}` is not an integer in i64 range")))
    }

    /// Parses a number as a finite `f64`.
    fn float(&mut self) -> Result<f64, DecodeError> {
        self.skip_ws();
        let text = self.number_str()?;
        let value: f64 = text
            .parse()
            .map_err(|_| self.error(format!("`{text}` is not a number")))?;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(self.error(format!("`{text}` overflows f64")))
        }
    }

    /// Decodes a value against `ty`.
    fn value(&mut self, ty: &Type, adts: &AdtTable) -> Result<WireValue, DecodeError> {
        self.skip_ws();
        match ty {
            Type::TyForall(_, _, body) => self.value(body, adts),
            Type::TyVar(_) => Err(self.error(String::from(
                "cannot decode at an uninstantiated type variable",
            ))),
            Type::TyFn(..) => {
                Err(self.error(format!("function type `{ty}` is not wire-representable")))
            }
            Type::TyTuple(elems) if elems.is_empty() => {
                self.literal("null")?;
                Ok(WireValue::Unit)
            }
            Type::TyTuple(elems) => {
                let items = self.array(elems.len(), |r, i| r.value(&elems[i], adts))?;
                Ok(WireValue::Tuple(items))
            }
            Type::TyRecord(fields) => {
                self.expect(b'{')?;
                let mut out = BTreeMap::new();
                for (i, (label, field_ty)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.expect(b',')?;
                    }
                    self.skip_ws();
                    self.key(label.as_str())?;
                    out.insert(label.clone(), self.value(field_ty, adts)?);
                }
                self.expect(b'}')?;
                Ok(WireValue::Record(out))
            }
            Type::TyCon(name, args) => self.constructed(name, args, adts),
        }
    }

    /// Decodes a `TyCon` value: a built-in, or an ADT through the table.
    fn constructed(
        &mut self,
        name: &Name,
        args: &[Type],
        adts: &AdtTable,
    ) -> Result<WireValue, DecodeError> {
        match (name.as_str(), args) {
            ("Int", []) => {
                let text = self.number_str()?;
                if text.contains(['.', 'e', 'E']) {
                    return Err(self.error(format!("`{text}` is not an integer")));
                }
                let value = text
                    .parse()
                    .map_err(|_| self.error(format!("`{text}` is not an integer in i64 range")))?;
                Ok(WireValue::Int(value))
            }
            ("Float", []) => Ok(WireValue::Float(self.float()?)),
            ("String", []) => Ok(WireValue::String(self.string()?)),
            ("List", [elem]) => {
                let mut items = Vec::new();
                self.expect(b'[')?;
                self.skip_ws();
                if self.peek() == Some(b']') {
                    self.pos += 1;
                    return Ok(WireValue::List(items));
                }
                loop {
                    items.push(self.value(elem, adts)?);
                    self.skip_ws();
                    if self.peek() == Some(b',') {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                self.expect(b']')?;
                Ok(WireValue::List(items))
            }
            _ => {
                let Some(ctors) = adts.ctors(name) else {
                    return Err(self.error(format!("no constructors known for type `{name}`")));
                };
                self.expect(b'{')?;
                self.skip_ws();
                self.key("ctor")?;
                let ctor = self.string()?;
                let Some((ctor_name, fields)) = ctors.iter().find(|(n, _)| n.as_str() == ctor)
                else {
                    return Err(self.error(format!("`{ctor}` is not a constructor of `{name}`")));
                };
                let instantiation: BTreeMap<u32, Type> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| (u32::try_from(i).unwrap_or(u32::MAX), a.clone()))
                    .collect();
                let rows = BTreeMap::new();
                self.expect(b',')?;
                self.skip_ws();
                self.key("args")?;
                let values = self.array(fields.len(), |r, i| {
                    r.value(&fields[i].substitute(&instantiation, &rows), adts)
                })?;
                self.expect(b'}')?;
                Ok(WireValue::Ctor(ctor_name.clone(), values))
            }
        }
    }

    /// Decodes a fixed-arity JSON array elementwise through `elem`.
    fn array(
        &mut self,
        len: usize,
        mut elem: impl FnMut(&mut Self, usize) -> Result<WireValue, DecodeError>,
    ) -> Result<Vec<WireValue>, DecodeError> {
        self.expect(b'[')?;
        let mut items = Vec::with_capacity(len);
        for i in 0..len {
            if i > 0 {
                self.expect(b',')?;
            }
            items.push(elem(self, i)?);
        }
        self.expect(b']')?;
        Ok(items)
    }

    /// Decodes the tagged `result` field: `{"ok":…}` against the signature's
    /// result type, or `{"err":…}` against its declared error types (first
    /// match wins).
    fn result(&mut self, sig: &ToolWireSig, adts: &AdtTable) -> Result<ToolResult, DecodeError> {
        self.expect(b'{')?;
        self.skip_ws();
        let tag = self.string()?;
        self.expect(b':')?;
        let result = match tag.as_str() {
            "ok" => ToolResult::Ok(self.value(&sig.result, adts)?),
            "err" => {
                let start = self.pos;
                let mut decoded = None;
                for err_ty in &sig.errors {
                    self.pos = start;
                    if let Ok(value) = self.value(err_ty, adts) {
                        decoded = Some(value);
                        break;
                    }
                }
                let Some(value) = decoded else {
                    self.pos = start;
                    return Err(self.error(String::from(
                        "err value matches none of the tool's declared error types",
                    )));
                };
                ToolResult::Err(value)
            }
            other => {
                return Err(self.error(format!(
                    "expected result tag `ok` or `err`, found `{other}`"
                )));
            }
        };
        self.expect(b'}')?;
        Ok(result)
    }

    /// Decodes a self-describing `meta` object.
    fn meta_object(&mut self) -> Result<BTreeMap<String, MetaValue>, DecodeError> {
        self.expect(b'{')?;
        let mut out = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(out);
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.expect(b':')?;
            out.insert(key, self.meta_value()?);
            self.skip_ws();
            if self.peek() == Some(b',') {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.expect(b'}')?;
        Ok(out)
    }

    /// Decodes one self-describing `meta` value.
    fn meta_value(&mut self) -> Result<MetaValue, DecodeError> {
        self.skip_ws();
        match self.peek() {
            Some(b'n') => {
                self.literal("null")?;
                Ok(MetaValue::Null)
            }
            Some(b't') => {
                self.literal("true")?;
                Ok(MetaValue::Bool(true))
            }
            Some(b'f') => {
                self.literal("false")?;
                Ok(MetaValue::Bool(false))
            }
            Some(b'"') => Ok(MetaValue::String(self.string()?)),
            Some(b'[') => {
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.peek() == Some(b']') {
                    self.pos += 1;
                    return Ok(MetaValue::Array(items));
                }
                loop {
                    items.push(self.meta_value()?);
                    self.skip_ws();
                    if self.peek() == Some(b',') {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                self.expect(b']')?;
                Ok(MetaValue::Array(items))
            }
            Some(b'{') => Ok(MetaValue::Object(self.meta_object()?)),
            _ => {
                let text = self.number_str()?;
                if text.contains(['.', 'e', 'E']) {
                    let value: f64 = text
                        .parse()
                        .map_err(|_| self.error(format!("`{text}` is not a number")))?;
                    Ok(MetaValue::Float(value))
                } else {
                    let value = text.parse().map_err(|_| {
                        self.error(format!("`{text}` is not an integer in i64 range"))
                    })?;
                    Ok(MetaValue::Int(value))
                }
            }
        }
    }
}

// ── replay ──────────────────────────────────────────────────────

/// Where and how a replay diverged from the log.
#[derive(Debug, Clone, PartialEq)]
pub enum Divergence {
    /// The log has no record at this position.
    Exhausted {
        /// The exhausted position.
        position: usize,
        /// The tool the program requested.
        requested: String,
    },
    /// The record at this position logs a different tool.
    ToolMismatch {
        /// Position of the mismatching record.
        position: usize,
        /// The tool the log recorded.
        logged: String,
        /// The tool the program requested.
        requested: String,
    },
    /// The record at this position logs the same tool with different args.
    ArgsMismatch {
        /// Position of the mismatching record.
        position: usize,
        /// The args the log recorded.
        logged: WireValue,
        /// The args the program requested.
        requested: WireValue,
    },
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exhausted {
                position,
                requested,
            } => write!(
                f,
                "replay diverged at position {position}: log exhausted, `{requested}` requested"
            ),
            Self::ToolMismatch {
                position,
                logged,
                requested,
            } => write!(
                f,
                "replay diverged at position {position}: log has `{logged}`, `{requested}` requested"
            ),
            Self::ArgsMismatch { position, .. } => write!(
                f,
                "replay diverged at position {position}: same tool, different args"
            ),
        }
    }
}

/// Replays one tool invocation against a log: the record at `position`
/// must match `tool` and `args` exactly, and its logged result — ok or
/// err — is returned. Matching is strictly sequential; the caller advances
/// `position` by one after each success. Any mismatch is a hard error:
/// keyed matching and live fall-through would reintroduce the
/// nondeterminism replay exists to remove.
pub fn replay<'log>(
    log: &'log [InvocationRecord],
    position: usize,
    tool: &str,
    args: &WireValue,
) -> Result<&'log ToolResult, Divergence> {
    let Some(record) = log.get(position) else {
        return Err(Divergence::Exhausted {
            position,
            requested: tool.to_owned(),
        });
    };
    if record.tool != tool {
        return Err(Divergence::ToolMismatch {
            position,
            logged: record.tool.clone(),
            requested: tool.to_owned(),
        });
    }
    if record.args != *args {
        return Err(Divergence::ArgsMismatch {
            position,
            logged: record.args.clone(),
            requested: args.clone(),
        });
    }
    Ok(&record.result)
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;
    use alloc::vec;

    use super::*;

    /// A record with every envelope field populated.
    fn sample_record() -> InvocationRecord {
        InvocationRecord {
            tool: String::from("ReadRepo"),
            args: WireValue::record([("path", WireValue::string("/home/user/repo"))]),
            result: ToolResult::Ok(WireValue::record([
                ("files", WireValue::List(vec![])),
                ("status", WireValue::string("clean")),
            ])),
            timestamp: String::from("2026-05-22T12:00:00.000Z"),
            caller: String::from("Planner.plan_repo"),
            meta: Some(BTreeMap::from([(
                String::from("duration_ms"),
                MetaValue::Int(42),
            )])),
        }
    }

    // -- canonical writing ---------------------------------------------

    #[test]
    fn envelope_fields_are_in_fixed_order() {
        let line = encode_record(&sample_record()).unwrap();
        assert_eq!(
            line,
            "{\"schema_version\":1,\"tool\":\"ReadRepo\",\
             \"args\":{\"path\":\"/home/user/repo\"},\
             \"result\":{\"ok\":{\"files\":[],\"status\":\"clean\"}},\
             \"timestamp\":\"2026-05-22T12:00:00.000Z\",\
             \"caller\":\"Planner.plan_repo\",\
             \"meta\":{\"duration_ms\":42}}"
        );
    }

    #[test]
    fn meta_is_omitted_when_absent() {
        let mut record = sample_record();
        record.meta = None;
        let line = encode_record(&record).unwrap();
        assert!(!line.contains("meta"), "absent meta must not be written");
        assert!(line.ends_with("\"caller\":\"Planner.plan_repo\"}"));
    }

    #[test]
    fn record_labels_write_sorted() {
        let value = WireValue::record([("zeta", WireValue::Int(1)), ("alpha", WireValue::Int(2))]);
        assert_eq!(encode_value(&value).unwrap(), "{\"alpha\":2,\"zeta\":1}");
    }

    #[test]
    fn adt_values_write_ctor_form() {
        let value = WireValue::ctor("Some", vec![WireValue::Int(1)]);
        assert_eq!(
            encode_value(&value).unwrap(),
            "{\"ctor\":\"Some\",\"args\":[1]}"
        );
        let value = WireValue::ctor("True", vec![]);
        assert_eq!(
            encode_value(&value).unwrap(),
            "{\"ctor\":\"True\",\"args\":[]}"
        );
    }

    #[test]
    fn unit_writes_null() {
        assert_eq!(encode_value(&WireValue::Unit).unwrap(), "null");
    }

    #[test]
    fn floats_write_shortest_round_trip() {
        assert_eq!(encode_value(&WireValue::Float(1.0)).unwrap(), "1");
        assert_eq!(encode_value(&WireValue::Float(0.1)).unwrap(), "0.1");
        assert_eq!(encode_value(&WireValue::Float(-2.5)).unwrap(), "-2.5");
    }

    #[test]
    fn non_finite_floats_are_rejected() {
        assert_eq!(
            encode_value(&WireValue::Float(f64::NAN)),
            Err(EncodeError::NonFiniteFloat)
        );
        assert_eq!(
            encode_value(&WireValue::Float(f64::INFINITY)),
            Err(EncodeError::NonFiniteFloat)
        );
    }

    #[test]
    fn ints_are_exact_at_i64_extremes() {
        assert_eq!(
            encode_value(&WireValue::Int(i64::MAX)).unwrap(),
            "9223372036854775807"
        );
        assert_eq!(
            encode_value(&WireValue::Int(i64::MIN)).unwrap(),
            "-9223372036854775808"
        );
    }

    #[test]
    fn strings_escape_controls_and_quotes() {
        let value = WireValue::string("a\"b\\c\nd\u{1}é");
        assert_eq!(encode_value(&value).unwrap(), "\"a\\\"b\\\\c\\nd\\u0001é\"");
    }

    #[test]
    fn bad_timestamps_are_rejected() {
        for bad in [
            "2026-05-22T12:00:00Z",       // no milliseconds
            "2026-05-22 12:00:00.000Z",   // no T
            "2026-05-22T12:00:00.000+00", // not Z
            "2026-13-22T12:00:00.000Z",   // month out of range
        ] {
            let mut record = sample_record();
            record.timestamp = String::from(bad);
            assert_eq!(
                encode_record(&record),
                Err(EncodeError::BadTimestamp(String::from(bad))),
                "`{bad}` should be rejected"
            );
        }
    }

    // -- the default sink ----------------------------------------------

    #[test]
    fn json_lines_sink_appends_lines() {
        let mut sink = JsonLinesSink::new();
        sink.emit(&sample_record()).unwrap();
        sink.emit(&sample_record()).unwrap();
        let log = sink.into_string();
        assert_eq!(log.lines().count(), 2, "one line per record");
        assert!(log.ends_with('\n'), "every line is terminated");
    }

    // -- decoding -------------------------------------------------------

    /// `Bool` and an `Option`-shaped ADT for decode tests.
    fn test_adts() -> AdtTable {
        let mut table = AdtTable::new();
        table.insert(
            Name::new("Bool"),
            vec![(Name::new("True"), vec![]), (Name::new("False"), vec![])],
        );
        table.insert(
            Name::new("Opt"),
            vec![
                (Name::new("Some"), vec![Type::TyVar(0)]),
                (Name::new("None"), vec![]),
            ],
        );
        table
    }

    #[test]
    fn decode_is_type_directed() {
        let adts = test_adts();
        assert_eq!(
            decode_value("1", &Type::int(), &adts).unwrap(),
            WireValue::Int(1)
        );
        assert_eq!(
            decode_value("1", &Type::float(), &adts).unwrap(),
            WireValue::Float(1.0)
        );
        assert_eq!(
            decode_value("null", &Type::tuple(vec![]), &adts).unwrap(),
            WireValue::Unit
        );
        assert_eq!(
            decode_value("[1,2]", &Type::list(Type::int()), &adts).unwrap(),
            WireValue::List(vec![WireValue::Int(1), WireValue::Int(2)])
        );
        assert_eq!(
            decode_value(
                "[1,\"a\"]",
                &Type::tuple(vec![Type::int(), Type::string()]),
                &adts
            )
            .unwrap(),
            WireValue::Tuple(vec![WireValue::Int(1), WireValue::string("a")])
        );
    }

    #[test]
    fn decode_instantiates_generic_ctor_fields() {
        let adts = test_adts();
        let ty = Type::con("Opt", vec![Type::string()]);
        assert_eq!(
            decode_value("{\"ctor\":\"Some\",\"args\":[\"x\"]}", &ty, &adts).unwrap(),
            WireValue::ctor("Some", vec![WireValue::string("x")])
        );
        // The field type is instantiated, so an Int payload is rejected.
        assert!(decode_value("{\"ctor\":\"Some\",\"args\":[1]}", &ty, &adts).is_err());
    }

    #[test]
    fn decode_rejects_unknown_ctor_and_wrong_labels() {
        let adts = test_adts();
        let ty = Type::con("Opt", vec![Type::int()]);
        assert!(decode_value("{\"ctor\":\"Sum\",\"args\":[]}", &ty, &adts).is_err());
        let rec_ty = Type::record([(Label::new("path"), Type::string())]);
        assert!(decode_value("{\"route\":\"x\"}", &rec_ty, &adts).is_err());
    }

    #[test]
    fn decode_rejects_float_syntax_for_int() {
        let adts = test_adts();
        assert!(decode_value("1.5", &Type::int(), &adts).is_err());
        assert!(decode_value("1e3", &Type::int(), &adts).is_err());
    }

    #[test]
    fn decode_rejects_wrong_schema_version() {
        let sig = ToolWireSig {
            args: Type::record([(Label::new("path"), Type::string())]),
            result: Type::string(),
            errors: vec![],
        };
        let line = "{\"schema_version\":2,\"tool\":\"T\",\"args\":{\"path\":\"p\"},\
                    \"result\":{\"ok\":\"v\"},\"timestamp\":\"2026-05-22T12:00:00.000Z\",\
                    \"caller\":\"M.f\"}";
        let err = decode_record(line, &sig, &AdtTable::new()).unwrap_err();
        assert!(err.message.contains("schema_version"), "{err}");
    }

    #[test]
    fn record_round_trips() {
        let record = sample_record();
        let line = encode_record(&record).unwrap();
        let sig = ToolWireSig {
            args: Type::record([(Label::new("path"), Type::string())]),
            result: Type::record([
                (Label::new("files"), Type::list(Type::string())),
                (Label::new("status"), Type::string()),
            ]),
            errors: vec![],
        };
        let decoded = decode_record(&line, &sig, &AdtTable::new()).unwrap();
        assert_eq!(decoded, record);
        assert_eq!(
            encode_record(&decoded).unwrap(),
            line,
            "re-encode is byte-identical"
        );
    }

    #[test]
    fn err_results_round_trip() {
        let mut record = sample_record();
        record.meta = None;
        record.result = ToolResult::Err(WireValue::ctor(
            "ParseError",
            vec![WireValue::string("bad json")],
        ));
        let line = encode_record(&record).unwrap();
        let mut adts = AdtTable::new();
        adts.insert(
            Name::new("ParseError"),
            vec![(Name::new("ParseError"), vec![Type::string()])],
        );
        let sig = ToolWireSig {
            args: Type::record([(Label::new("path"), Type::string())]),
            result: Type::record([
                (Label::new("files"), Type::list(Type::string())),
                (Label::new("status"), Type::string()),
            ]),
            errors: vec![Type::con("ParseError", vec![])],
        };
        let decoded = decode_record(&line, &sig, &adts).unwrap();
        assert_eq!(decoded, record);
    }

    // -- replay ----------------------------------------------------------

    #[test]
    fn replay_returns_logged_results_in_order() {
        let record = sample_record();
        let log = vec![record.clone()];
        let result = replay(&log, 0, "ReadRepo", &record.args).unwrap();
        assert_eq!(result, &record.result);
    }

    #[test]
    fn replay_diverges_structurally() {
        let record = sample_record();
        let log = vec![record.clone()];
        assert_eq!(
            replay(&log, 1, "ReadRepo", &record.args),
            Err(Divergence::Exhausted {
                position: 1,
                requested: String::from("ReadRepo"),
            })
        );
        assert!(matches!(
            replay(&log, 0, "CreateTicket", &record.args),
            Err(Divergence::ToolMismatch { position: 0, .. })
        ));
        assert!(matches!(
            replay(&log, 0, "ReadRepo", &WireValue::Unit),
            Err(Divergence::ArgsMismatch { position: 0, .. })
        ));
    }

    #[test]
    fn divergence_renders_position_and_tools() {
        let divergence = Divergence::ToolMismatch {
            position: 3,
            logged: String::from("CreateTicket"),
            requested: String::from("ReadRepo"),
        };
        assert_eq!(
            divergence.to_string(),
            "replay diverged at position 3: log has `CreateTicket`, `ReadRepo` requested"
        );
    }
}

// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Byte-offset ↔ LSP position conversion.
//!
//! Spans are byte offsets into the source; LSP positions are zero-based
//! line/character pairs with characters counted in UTF-16 code units.

use tower_lsp::lsp_types::Position;

/// Precomputed line starts for one source text.
#[derive(Debug)]
pub(crate) struct LineIndex {
    /// Byte offset of the first byte of each line, starting with `0`.
    line_starts: Vec<u32>,
    /// Total source length in bytes.
    len: u32,
}

impl LineIndex {
    /// Indexes `source`.
    #[must_use]
    pub(crate) fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(u32::try_from(i + 1).unwrap_or(u32::MAX));
            }
        }
        Self {
            line_starts,
            len: u32::try_from(source.len()).unwrap_or(u32::MAX),
        }
    }

    /// The position of byte `offset` in `source` (clamped to the text end).
    #[must_use]
    pub(crate) fn position(&self, source: &str, offset: u32) -> Position {
        let offset = offset.min(self.len);
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts[line] as usize;
        let character = source[line_start..offset as usize]
            .chars()
            .map(utf16_len)
            .sum();
        Position {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            character,
        }
    }

    /// The byte offset of `position` in `source`. `None` when the line does
    /// not exist; a character past the line end clamps to the line end.
    #[must_use]
    pub(crate) fn offset(&self, source: &str, position: Position) -> Option<u32> {
        let line_start = *self.line_starts.get(position.line as usize)? as usize;
        let line_end = self
            .line_starts
            .get(position.line as usize + 1)
            .map_or(self.len as usize, |&next| next as usize);
        let mut remaining = position.character;
        let mut offset = line_start;
        for c in source[line_start..line_end].chars() {
            if remaining == 0 || c == '\n' {
                break;
            }
            remaining = remaining.saturating_sub(utf16_len(c));
            offset += c.len_utf8();
        }
        Some(u32::try_from(offset).unwrap_or(u32::MAX))
    }
}

/// UTF-16 length of `c`: 1 unit, or 2 for supplementary-plane characters.
fn utf16_len(c: char) -> u32 {
    if c.len_utf16() == 2 { 2 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_ascii() {
        let source = "fn a() = 1\nfn b() = 2\n";
        let index = LineIndex::new(source);
        let position = index.position(source, 14);
        assert_eq!(position, Position::new(1, 3), "byte 14 is `b` on line 1");
        assert_eq!(index.offset(source, position), Some(14), "inverse mapping");
    }

    #[test]
    fn counts_utf16_units() {
        // `→` is 3 bytes, 1 UTF-16 unit; `𝕏` is 4 bytes, 2 UTF-16 units.
        let source = "→𝕏x";
        let index = LineIndex::new(source);
        assert_eq!(
            index.position(source, 7),
            Position::new(0, 3),
            "x follows one 1-unit and one 2-unit char"
        );
        assert_eq!(
            index.offset(source, Position::new(0, 3)),
            Some(7),
            "inverse mapping"
        );
    }

    #[test]
    fn clamps_past_line_end() {
        let source = "ab\ncd";
        let index = LineIndex::new(source);
        assert_eq!(
            index.offset(source, Position::new(0, 99)),
            Some(2),
            "clamps to before the newline"
        );
        assert_eq!(
            index.offset(source, Position::new(9, 0)),
            None,
            "missing line"
        );
    }
}

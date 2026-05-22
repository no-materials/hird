---
id: hir-p78v
status: open
deps: [hir-2upi]
links: []
created: 2026-05-22T21:36:15Z
type: task
priority: 2
assignee: nomaterials
parent: hir-cn1r
tags: [phase-1, lexer, testing]
---
# Lexer snapshot tests and error recovery

Comprehensive test suite for hird-lex using insta for snapshot testing.

Test categories:
1. **Keyword recognition**: each keyword lexes to its variant.
2. **Operator tokens**: all operators including Unicode forms.
3. **Unicode normalization pairs**: verify -> and → produce identical streams,
   same for =>/⇒ and \/λ.
4. **String literals**: simple strings, escape sequences, multiline (if supported).
5. **Numeric literals**: integers, floats, underscores in numbers.
6. **Comments**: line comments, block comments, nested block comments.
7. **Canonical naming violations**: camelCase value, lowercase type, multi-char
   type variable — each produces appropriate diagnostic.
8. **Error recovery**: unterminated string (lexer continues, error token emitted),
   invalid Unicode byte sequence, unexpected character.
9. **Full program lexing**: a small but complete Hirð program lexes to the expected
   token stream.

Use `insta` for all snapshot tests. Snapshots should be committed and reviewable
in PRs.

Add property tests with `proptest`: random valid identifier strings lex and
round-trip through the canonical naming check.

## Acceptance Criteria

- insta snapshot tests for all 9 categories listed above.
- At least 30 snapshot tests total.
- proptest for identifier round-tripping.
- All tests pass; snapshots committed.
- Test coverage: every token variant appears in at least one snapshot.


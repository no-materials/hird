---
id: hir-2upi
status: closed
deps: [hir-8unj]
links: []
created: 2026-05-22T21:36:07Z
type: task
priority: 1
assignee: nomaterials
parent: hir-cn1r
tags: [phase-1, lexer]
---
# Token enum and Unicode normalization

Implement the core lexer in hird-lex:

1. **Token enum** with variants for all v0.1 syntax: keywords (let, fn, match,
   type, actor, supervisor, effect, tool, handle, spawn, send, request, use,
   module, pub, extern, if, then, else), identifiers, integer/float/string
   literals, operators (arithmetic, comparison, arrow ->/→, fat-arrow =>/⇒,
   pipe |, bang !, dot, colon, double-colon ::, lambda \/λ), delimiters,
   comments, EOF, Error.

2. **Span tracking**: every token carries (start_byte, end_byte, source_id).
   Define a Span type that is cheap to copy and store.

3. **Unicode normalization**: at lex time, normalize ASCII operator sequences
   to canonical Unicode forms. Both -> and → produce the same Arrow token.
   Both => and ⇒ produce the same FatArrow token. Both \ and λ produce the
   same Lambda token. The canonical form is the Unicode version. This is a
   save-time normalization inherited from a sibling project.

4. **Canonical naming pre-check**: identifiers are checked against naming rules
   at lex time. snake_case for values, PascalCase for types, single lowercase
   for type variables. Violations produce diagnostic tokens. Decision: whether
   this is an error or warning — lean error for v0.1 to enforce consistency.

5. **Error recovery**: on malformed input (unterminated string, invalid byte
   sequence), emit an Error token with a diagnostic and continue lexing.

The lexer should be `#![no_std]` compatible, operating on `&str` input.
Use no external parsing libraries — the lexer is hand-written for performance
and control.

## Acceptance Criteria

- Token enum covers all v0.1 keywords, operators, literals, delimiters.
- Span type defined, every token carries a span.
- -> and → lex to identical tokens; => and ⇒ lex to identical tokens; \ and λ lex to identical tokens.
- Canonical naming violations produce diagnostics.
- Error tokens emitted for malformed input; lexer continues past errors.
- Unit tests for each token variant.
- No external dependencies for core lexing logic.


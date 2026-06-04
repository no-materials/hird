# Parser Diagnostic Codes

Index of the diagnostic codes emitted by `hird-parse`. Codes use a `P` prefix
(parser) followed by a four-digit number. Each diagnostic carries a code, a
source span, a human-readable message, and an optional help suggestion.

| Code  | Meaning                                                              |
|-------|---------------------------------------------------------------------|
| P0001 | Expected a specific token, but found a different one.               |
| P0002 | Unexpected token in this position; wrapped in an error node.        |
| P0003 | Malformed type annotation.                                          |
| P0004 | Nesting depth limit exceeded.                                       |
| P0005 | Non-associative operator used in a chain (parenthesise instead).    |

Codes are stable identifiers: once assigned, a code keeps its meaning. New
diagnostics take the next free number rather than reusing a retired one.

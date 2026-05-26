// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![expect(missing_docs, reason = "test suite")]

use hird_lex::{Lexer, TokenKind};
use proptest::prelude::*;

proptest! {
    #[test]
    fn snake_case_round_trips(s in "[a-z][a-z0-9_]{0,20}") {
        let tokens: Vec<_> = Lexer::new(&s, 0).collect();
        prop_assert_eq!(tokens.len(), 1, "expected single token for {:?}", s);
        let kind = tokens[0].kind;
        prop_assert!(
            kind == TokenKind::Ident || kind.is_keyword(),
            "expected Ident or keyword for {:?}, got {:?}", s, kind,
        );
        prop_assert_eq!(tokens[0].span.text(&s), s.as_str());
    }

    #[test]
    fn pascal_case_round_trips(s in "[A-Z][a-zA-Z0-9]{0,20}") {
        let tokens: Vec<_> = Lexer::new(&s, 0).collect();
        prop_assert_eq!(tokens.len(), 1, "expected single token for {:?}", s);
        prop_assert_eq!(tokens[0].kind, TokenKind::Ident);
        prop_assert_eq!(tokens[0].span.text(&s), s.as_str());
    }

    #[test]
    fn underscore_prefixed_snake_case_round_trips(s in "_+[a-z][a-z0-9_]{0,20}") {
        let tokens: Vec<_> = Lexer::new(&s, 0).collect();
        prop_assert_eq!(tokens.len(), 1, "expected single token for {:?}", s);
        prop_assert_eq!(tokens[0].kind, TokenKind::Ident);
        prop_assert_eq!(tokens[0].span.text(&s), s.as_str());
    }
}

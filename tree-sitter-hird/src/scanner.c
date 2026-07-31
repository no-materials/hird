#include "tree_sitter/parser.h"

// Hirð block comments nest (`/* outer /* inner */ still outer */`), which no
// regular token can express. Everything else lexes in the generated parser.

enum TokenType {
  BLOCK_COMMENT,
};

void *tree_sitter_hird_external_scanner_create(void) { return NULL; }

void tree_sitter_hird_external_scanner_destroy(void *payload) { (void)payload; }

unsigned tree_sitter_hird_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}

void tree_sitter_hird_external_scanner_deserialize(void *payload, const char *buffer,
                                                   unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

static bool is_space(int32_t c) {
  return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v';
}

bool tree_sitter_hird_external_scanner_scan(void *payload, TSLexer *lexer,
                                            const bool *valid_symbols) {
  (void)payload;

  if (!valid_symbols[BLOCK_COMMENT]) {
    return false;
  }

  while (is_space(lexer->lookahead)) {
    lexer->advance(lexer, true);
  }

  if (lexer->lookahead != '/') {
    return false;
  }
  lexer->advance(lexer, false);
  if (lexer->lookahead != '*') {
    return false;
  }
  lexer->advance(lexer, false);

  unsigned depth = 1;
  while (depth > 0) {
    if (lexer->eof(lexer)) {
      return false;
    }
    if (lexer->lookahead == '/') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '*') {
        lexer->advance(lexer, false);
        depth++;
      }
    } else if (lexer->lookahead == '*') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '/') {
        lexer->advance(lexer, false);
        depth--;
      }
    } else {
      lexer->advance(lexer, false);
    }
  }

  lexer->result_symbol = BLOCK_COMMENT;
  return true;
}

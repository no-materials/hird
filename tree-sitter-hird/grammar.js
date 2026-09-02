// Tree-sitter grammar for Hirð v0.1.
//
// Tracks `docs/grammar.md`, the normative surface grammar. Where the two
// could drift, the reference parser (`crates/hird-parse`) wins: this
// grammar exists to highlight real buffers, not to define the language.
//
// Naming is part of the grammar here, as it is in the lexer: `identifier`
// is snake_case, `type_identifier` is PascalCase, and the two are separate
// tokens so a query can tell a constructor from a binding without help.

const PREC = {
  // `;` binds looser than every operator but tighter than the bodies of
  // `let`/`if`/`match`/`handle`, so `let x = e in a; b` sequences inside
  // the body and `x + y; z` sequences the sum.
  sequence: 1,
  or: 2,
  and: 3,
  compare: 4,
  add: 5,
  mul: 6,
  application: 7,
  field: 8,
};

// The lexer canonicalises each ASCII spelling to its Unicode form, so both
// spellings must reach the same production.
const ARROW = ['->', '→'];
const LAMBDA = ['\\', 'λ'];
const AND = ['&&', '∧'];
const OR = ['||', '∨'];

/** A non-empty comma-separated list with an optional trailing comma. */
function commaList(rule) {
  return seq(rule, repeat(seq(',', rule)), optional(','));
}

module.exports = grammar({
  name: 'hird',

  word: ($) => $.identifier,

  extras: ($) => [/\s/, $.line_comment, $.block_comment],

  // Block comments nest, which no regular token can express.
  externals: ($) => [$.block_comment],

  conflicts: ($) => [
    // `use M.{a}` and `use M.N` share the `.` after a path segment.
    [$.path],
  ],

  rules: {
    source_file: ($) => seq(optional($.module_declaration), repeat($._declaration)),

    // ── declarations ────────────────────────────────────────────

    module_declaration: ($) => seq('module', field('name', $.type_identifier)),

    _declaration: ($) =>
      choice(
        $.use_declaration,
        $.function_declaration,
        $.type_declaration,
        $.actor_declaration,
        $.supervisor_declaration,
        $.effect_declaration,
        $.tool_declaration,
        $.extern_declaration,
      ),

    use_declaration: ($) =>
      seq(
        'use',
        field('path', $.path),
        optional(choice($.use_group, seq('as', field('alias', $.type_identifier)))),
      ),

    path: ($) => seq($.type_identifier, repeat(seq('.', $.type_identifier))),

    use_group: ($) => seq('.', '{', commaList(choice($.identifier, $.type_identifier)), '}'),

    function_declaration: ($) =>
      seq(
        optional('pub'),
        'fn',
        field('name', $.identifier),
        field('parameters', $.parameter_list),
        optional(field('return_type', $.return_type)),
        optional(field('effects', $.effect_annotation)),
        '=',
        field('body', $._expression),
      ),

    parameter_list: ($) => seq('(', optional(commaList($.parameter)), ')'),

    parameter: ($) => seq(field('name', $.identifier), ':', field('type', $._type)),

    return_type: ($) => seq(choice(...ARROW), $._type),

    effect_annotation: ($) => seq('!', '{', optional(commaList(field('effect', $._type))), '}'),

    type_declaration: ($) =>
      seq(
        optional(seq('pub', optional('opaque'))),
        'type',
        field('name', $.type_identifier),
        optional(field('type_parameters', $.type_parameters)),
        '=',
        $._constructors,
      ),

    _constructors: ($) => seq(optional('|'), $.constructor, repeat(seq('|', $.constructor))),

    constructor: ($) =>
      seq(field('name', $.type_identifier), optional(seq('(', optional(commaList($._type)), ')'))),

    type_parameters: ($) => seq('<', commaList($._type_variable), '>'),

    actor_declaration: ($) =>
      seq(
        optional('pub'),
        'actor',
        field('name', $.type_identifier),
        '{',
        optional(commaList($._actor_member)),
        '}',
        optional(field('effects', $.effect_annotation)),
      ),

    _actor_member: ($) => choice($.actor_field, $.actor_handler),

    // Shape, not field name, picks the form: a signature with a body is
    // `init`, a type with an ADT tail is `message`, a bare type is `state`.
    actor_field: ($) =>
      seq(
        field('name', $.identifier),
        ':',
        choice(
          seq(field('signature', $.function_signature), '=', field('body', $._expression)),
          seq(field('type', $._type), optional(seq('=', $._constructors))),
        ),
      ),

    function_signature: ($) =>
      seq(
        'fn',
        field('parameters', $.parameter_list),
        optional(field('return_type', $.return_type)),
        optional(field('effects', $.effect_annotation)),
      ),

    actor_handler: ($) =>
      seq(
        'handle',
        field('message', $._pattern),
        ',',
        field('state', $._pattern),
        optional(field('return_type', $.return_type)),
        optional(field('effects', $.effect_annotation)),
        '=',
        field('body', $._expression),
      ),

    supervisor_declaration: ($) =>
      seq(
        optional('pub'),
        'supervisor',
        field('name', $.type_identifier),
        '{',
        optional(commaList($.supervisor_field)),
        '}',
      ),

    supervisor_field: ($) => seq(field('name', $.identifier), ':', field('value', $._expression)),

    effect_declaration: ($) =>
      seq(
        optional('pub'),
        'effect',
        field('name', $.type_identifier),
        optional(field('type_parameters', $.type_parameters)),
      ),

    tool_declaration: ($) =>
      seq(
        optional('pub'),
        'tool',
        field('name', $.type_identifier),
        optional(field('type_parameters', $.type_parameters)),
        ':',
        field('input', $._app_type),
        choice(...ARROW),
        field('output', $._type),
        optional(field('effects', $.effect_annotation)),
      ),

    extern_declaration: ($) =>
      seq(
        'extern',
        'fn',
        field('name', $.identifier),
        field('parameters', $.parameter_list),
        optional(field('return_type', $.return_type)),
      ),

    // ── types ───────────────────────────────────────────────────

    _type: ($) => choice($.function_type, $._app_type),

    function_type: ($) =>
      seq(
        $._app_type,
        repeat1(
          seq(choice(...ARROW), $._app_type, optional(field('effects', $.effect_annotation))),
        ),
      ),

    _app_type: ($) => choice($.generic_type, $._atom_type),

    generic_type: ($) => seq(field('name', $._atom_type), field('arguments', $.type_arguments)),

    type_arguments: ($) => seq('<', commaList($._type), '>'),

    _atom_type: ($) =>
      choice(
        $.type_identifier,
        $._type_variable,
        $.record_type,
        $.unit_type,
        $.parenthesized_type,
        $.tuple_type,
      ),

    // A `{` where a type is expected always begins a record type.
    record_type: ($) => seq('{', optional(commaList($.record_type_field)), '}'),

    record_type_field: ($) => seq(field('name', $.identifier), ':', field('type', $._type)),

    unit_type: ($) => seq('(', ')'),

    parenthesized_type: ($) => seq('(', $._type, ')'),

    tuple_type: ($) => seq('(', $._type, repeat1(seq(',', $._type)), optional(','), ')'),

    // ── expressions ─────────────────────────────────────────────

    _expression: ($) =>
      choice(
        $.sequence_expression,
        $.let_expression,
        $.lambda_expression,
        $.if_expression,
        $.match_expression,
        $.handle_expression,
        $.install_expression,
        $.spawn_expression,
        $.supervise_expression,
        $.child_expression,
        $.send_expression,
        $.request_expression,
        $.reply_expression,
        $.crash_expression,
        $.binary_expression,
        $.application,
        $.field_expression,
        $._atom,
      ),

    _atom: ($) =>
      choice(
        $.identifier,
        $.type_identifier,
        $.integer,
        $.float,
        $.string,
        $.unit,
        $.parenthesized_expression,
        $.tuple,
        $.list,
        $.record,
      ),

    sequence_expression: ($) =>
      prec.right(
        PREC.sequence,
        seq(field('first', $._expression), ';', field('rest', $._expression)),
      ),

    let_expression: ($) =>
      seq(
        'let',
        field('pattern', $._pattern),
        optional(seq(':', field('type', $._type))),
        '=',
        field('value', $._expression),
        'in',
        field('body', $._expression),
      ),

    lambda_expression: ($) =>
      seq(
        choice(...LAMBDA),
        repeat1(field('parameter', $.identifier)),
        choice(...ARROW),
        field('body', $._expression),
      ),

    if_expression: ($) =>
      seq(
        'if',
        field('condition', $._expression),
        'then',
        field('consequence', $._expression),
        'else',
        field('alternative', $._expression),
      ),

    // The scrutinee may not itself begin with `{`: that brace opens the arm
    // block. A record literal there would need parenthesising.
    match_expression: ($) =>
      seq('match', field('value', $._expression), '{', optional(commaList($.match_arm)), '}'),

    match_arm: ($) =>
      seq(field('pattern', $._pattern), choice(...ARROW), field('body', $._expression)),

    handle_expression: ($) => seq('handle', $._handler_arms, 'in', field('body', $._expression)),

    install_expression: ($) => seq('install', $._handler_arms, 'in', field('body', $._expression)),

    _handler_arms: ($) => seq('{', optional(commaList($.handle_arm)), '}'),

    handle_arm: ($) =>
      seq(field('effect', $._app_type), choice(...ARROW), field('handler', $._expression)),

    // Actor and supervisor names are not first-class values, so these keyword
    // forms take a name rather than an expression in those positions.
    spawn_expression: ($) =>
      seq(
        'spawn',
        '(',
        field('actor', $.type_identifier),
        repeat(seq(',', field('argument', $._expression))),
        optional(','),
        ')',
      ),

    supervise_expression: ($) =>
      seq('supervise', '(', field('supervisor', $.type_identifier), ')'),

    child_expression: ($) =>
      seq(
        'child',
        '(',
        field('supervisor', $.type_identifier),
        ',',
        field('id', $.identifier),
        ')',
      ),

    send_expression: ($) =>
      seq('send', '(', field('target', $._expression), ',', field('message', $._expression), ')'),

    request_expression: ($) =>
      seq(
        'request',
        '(',
        field('target', $._expression),
        ',',
        field('message', $._expression),
        ')',
      ),

    reply_expression: ($) =>
      seq('reply', '(', field('target', $._expression), ',', field('value', $._expression), ')'),

    crash_expression: ($) =>
      seq(choice('crash', 'panic'), '!', '(', field('message', $._expression), ')'),

    binary_expression: ($) =>
      choice(
        ...[
          [PREC.or, OR],
          [PREC.and, AND],
          [PREC.compare, ['==', '!=', '<', '>', '<=', '>=']],
          [PREC.add, ['+', '-']],
          [PREC.mul, ['*', '/']],
        ].map(([precedence, operators]) =>
          prec.left(
            precedence,
            seq(
              field('left', $._expression),
              field('operator', choice(...operators)),
              field('right', $._expression),
            ),
          ),
        ),
      ),

    application: ($) =>
      prec.left(
        PREC.application,
        seq(field('function', $._expression), field('argument', $._argument)),
      ),

    // A `{` never begins an application argument: record-literal arguments
    // must be parenthesised, which keeps `match e { … }` unambiguous.
    _argument: ($) =>
      choice(
        $.identifier,
        $.type_identifier,
        $.integer,
        $.float,
        $.string,
        $.unit,
        $.parenthesized_expression,
        $.tuple,
        $.list,
        alias($._argument_field, $.field_expression),
      ),

    _argument_field: ($) =>
      prec.left(PREC.field, seq(field('value', $._argument), '.', field('field', $.identifier))),

    // Qualified access (`Mod.member`) has no dedicated production: it is
    // field access on a PascalCase receiver, resolved in the checker.
    field_expression: ($) =>
      prec.left(PREC.field, seq(field('value', $._expression), '.', field('field', $.identifier))),

    unit: ($) => seq('(', ')'),

    parenthesized_expression: ($) => seq('(', $._expression, ')'),

    tuple: ($) => seq('(', $._expression, repeat1(seq(',', $._expression)), optional(','), ')'),

    list: ($) => seq('[', optional(commaList($._expression)), ']'),

    record: ($) => seq('{', optional(commaList($.record_field)), '}'),

    // A keyword spelling is a legal field name (`actor: Planner` in a child
    // spec); the `name :` shape leaves no ambiguity, and no keyword is valid
    // here, so the word token falls back to `identifier`.
    record_field: ($) => seq(field('name', $.identifier), ':', field('value', $._expression)),

    // ── patterns ────────────────────────────────────────────────

    _pattern: ($) =>
      choice(
        $.constructor_pattern,
        $.tuple_pattern,
        $._parenthesized_pattern,
        $.integer,
        $.float,
        $.string,
        $.wildcard_pattern,
        $._bind_pattern,
      ),

    _parenthesized_pattern: ($) => seq('(', $._pattern, ')'),

    constructor_pattern: ($) =>
      seq(
        field('name', $.type_identifier),
        optional(seq('(', optional(commaList($._pattern)), ')')),
      ),

    tuple_pattern: ($) =>
      choice(
        seq('(', ')'),
        seq('(', $._pattern, repeat1(seq(',', $._pattern)), optional(','), ')'),
      ),

    wildcard_pattern: ($) => '_',

    _bind_pattern: ($) => alias($.identifier, $.bind_pattern),

    // ── tokens ──────────────────────────────────────────────────

    // Canonical naming, as the lexer enforces it: snake_case values,
    // PascalCase types. `_`, `__`, … are identifiers.
    identifier: ($) => /[a-z][a-z0-9_]*|_+[a-z0-9][a-z0-9_]*|_+/,

    type_identifier: ($) => /_*[A-Z][a-zA-Z0-9]*/,

    _type_variable: ($) => alias($.identifier, $.type_variable),

    integer: ($) => /[0-9]+/,

    float: ($) => /[0-9]+\.[0-9]+/,

    string: ($) => token(seq('"', repeat(choice(/[^"\\\n]/, /\\[^\n]/)), '"')),

    line_comment: ($) => token(seq('//', /[^\n]*/)),
  },
});

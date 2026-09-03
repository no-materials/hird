; Hirð syntax highlighting.
;
; Later patterns win, so this file reads general to specific: bare names
; first, then the role each name plays. Effect heads and the names of
; `effect`/`tool` declarations share @attribute, which is what makes an
; effect row read as an annotation rather than as more type text.

; ── comments ────────────────────────────────────────────────

[
  (line_comment)
  (block_comment)
] @comment @spell

; ── literals ────────────────────────────────────────────────

(string) @string

(integer) @number

(float) @number.float

; ── names ───────────────────────────────────────────────────

(identifier) @variable

(type_identifier) @type

(type_variable) @variable.parameter

(wildcard_pattern) @variable.builtin

((type_identifier) @type.builtin
  (#any-of? @type.builtin
    "Int" "Float" "String" "Bool" "List" "Option" "Pid" "ReplyTo"))

((type_identifier) @constant.builtin
  (#any-of? @constant.builtin "True" "False"))

; ── declarations ────────────────────────────────────────────

(module_declaration
  name: (type_identifier) @module)

(path
  (type_identifier) @module)

(use_declaration
  alias: (type_identifier) @module)

(use_group
  (identifier) @function)

(function_declaration
  name: (identifier) @function)

(extern_declaration
  name: (identifier) @function)

(type_declaration
  name: (type_identifier) @type.definition)

(actor_declaration
  name: (type_identifier) @type.definition)

(supervisor_declaration
  name: (type_identifier) @type.definition)

(constructor
  name: (type_identifier) @constructor)

(constructor_pattern
  name: (type_identifier) @constructor)

; ── bindings and members ────────────────────────────────────

(parameter
  name: (identifier) @variable.parameter)

(lambda_expression
  parameter: (identifier) @variable.parameter)

(bind_pattern) @variable

(record_field
  name: (identifier) @property)

(record_type_field
  name: (identifier) @property)

(field_expression
  field: (identifier) @property)

(supervisor_field
  name: (identifier) @property)

(actor_field
  name: (identifier) @property)

(child_expression
  id: (identifier) @property)

; ── applications ────────────────────────────────────────────

(application
  function: (identifier) @function.call)

(application
  function: (type_identifier) @constructor)

; ── effects and tools ───────────────────────────────────────

(effect_declaration
  name: (type_identifier) @attribute)

(tool_declaration
  name: (type_identifier) @attribute)

(effect_annotation
  effect: [
    (type_identifier)
    (type_variable)
  ] @attribute)

(effect_annotation
  effect: (generic_type
    name: (type_identifier) @attribute))

(handle_arm
  effect: (type_identifier) @attribute)

(handle_arm
  effect: (generic_type
    name: (type_identifier) @attribute))

; ── keywords ────────────────────────────────────────────────

[
  "module"
  "use"
  "as"
] @keyword.import

[
  "pub"
  "opaque"
  "extern"
] @keyword.modifier

"fn" @keyword.function

[
  "type"
  "alias"
  "effect"
  "tool"
  "actor"
  "supervisor"
] @keyword.type

[
  "let"
  "in"
  "handle"
  "install"
] @keyword

[
  "if"
  "then"
  "else"
  "match"
] @keyword.conditional

[
  "spawn"
  "supervise"
  "child"
  "send"
  "request"
  "reply"
] @keyword.coroutine

[
  "crash"
  "panic"
] @keyword.exception

; ── operators and punctuation ───────────────────────────────

[
  "="
  "+"
  "-"
  "*"
  "/"
  "=="
  "!="
  "<"
  ">"
  "<="
  ">="
  "&&"
  "∧"
  "||"
  "∨"
  "->"
  "→"
  "\\"
  "λ"
] @operator

[
  "("
  ")"
  "{"
  "}"
  "["
  "]"
] @punctuation.bracket

[
  ","
  ":"
  "."
  "|"
] @punctuation.delimiter

; Angle brackets delimit a type application, not a comparison.
(type_arguments
  [
    "<"
    ">"
  ] @punctuation.bracket)

(type_parameters
  [
    "<"
    ">"
  ] @punctuation.bracket)

; The `!` opens an effect row or completes `crash!`; neither is an operator.
(effect_annotation
  "!" @attribute)

(crash_expression
  "!" @keyword.exception)

; Unit is a value and a type, not an empty bracket pair.
(unit) @constant.builtin

(unit_type) @type.builtin

; Hirð indentation.
;
; Every construct that opens a delimited group indents its contents one
; level; the closing delimiter comes back out.

[
  (actor_declaration)
  (supervisor_declaration)
  (match_expression)
  (handle_expression)
  (install_expression)
  (parameter_list)
  (type_parameters)
  (type_arguments)
  (effect_annotation)
  (record)
  (record_type)
  (list)
  (tuple)
  (tuple_type)
  (tuple_pattern)
  (parenthesized_expression)
  (parenthesized_type)
] @indent.begin

[
  ")"
  "]"
  "}"
] @indent.branch

(ERROR) @indent.auto

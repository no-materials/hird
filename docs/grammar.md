# Hird v0.1 Grammar

Formal grammar for the Hird surface syntax. This document is a first-class
project artifact included in LLM context for code generation and analysis.

## Notation

```
foo        keyword or terminal
FOO        token class (IDENT, INT, FLOAT, STR)
foo?       optional
foo*       zero or more
foo+       one or more
foo | bar  alternation
( ... )    grouping
'→'        Unicode literal (lexer normalises -> to →)
'⇒'        Unicode literal (lexer normalises => to ⇒)
'λ'        Unicode literal (lexer normalises \ to λ)
```

Whitespace and comments are implicit between tokens. The grammar describes
the abstract syntax; the CST preserves all trivia.

## Module Level

```
source_file  ::= module_decl? top_item*

module_decl  ::= 'module' IDENT

top_item     ::= use_decl
               | fn_decl
               | type_decl
               | actor_decl
               | supervisor_decl
               | effect_decl
               | tool_decl
               | extern_decl
```

## Use Declarations

```
use_decl     ::= 'use' path ( 'as' IDENT )?

path         ::= IDENT ( '::' IDENT )*
```

## Function Declarations

```
fn_decl      ::= visibility? 'fn' IDENT '(' param_list? ')' return_type?
                  effect_ann? '=' expr

visibility   ::= 'pub'

param_list   ::= param ( ',' param )* ','?

param        ::= IDENT ':' type_expr

return_type  ::= '→' type_expr

effect_ann   ::= '!' '{' effect_list? '}'

effect_list  ::= type_expr ( ',' type_expr )* ','?
```

## Type Declarations (ADTs)

```
type_decl    ::= visibility? 'type' IDENT type_params? '=' constructors

type_params  ::= '<' IDENT ( ',' IDENT )* ','? '>'

constructors ::= '|'? constructor ( '|' constructor )*

constructor  ::= IDENT ( '(' field_list ')' )?

field_list   ::= type_expr ( ',' type_expr )* ','?
```

## Actor Declarations

```
actor_decl    ::= visibility? 'actor' IDENT '{' actor_body? '}' effect_ann?

actor_body    ::= actor_member ( ',' actor_member )* ','?

actor_member  ::= actor_field | actor_handler

actor_field   ::= IDENT ':' actor_value

actor_value   ::= fn_sig '=' expr | type_expr ( '=' constructors )?

fn_sig        ::= 'fn' '(' param_list? ')' return_type? effect_ann?

actor_handler ::= 'handle' pattern ',' pattern return_type? effect_ann? '=' expr
```

A handler binds the message pattern, then the current state as a trailing
comma-separated pattern. Handler and `init` bodies follow the uniform
bare-body rule (`= e`); braces never wrap a body.

## Supervisor Declarations (syntax only)

Semantic validation deferred to Phase 8.

```
supervisor_decl  ::= visibility? 'supervisor' IDENT '{' supervisor_body? '}'

supervisor_body  ::= supervisor_field ( ',' supervisor_field )* ','?

supervisor_field ::= IDENT ':' expr
```

## Effect Declarations (syntax only)

Semantic validation deferred to Phase 5.

```
effect_decl  ::= visibility? 'effect' IDENT type_params?
```

## Tool Declarations (syntax only)

Semantic validation deferred to Phase 6.

```
tool_decl    ::= visibility? 'tool' IDENT ':' type_expr '→' type_expr
```

## Extern Declarations

```
extern_decl  ::= 'extern' 'fn' IDENT '(' param_list? ')' return_type?
```

## Expressions

```
expr         ::= let_expr
               | lambda_expr
               | match_expr
               | if_expr
               | handle_expr
               | spawn_expr
               | infix_expr

let_expr     ::= 'let' IDENT ( ':' type_expr )? '=' expr 'in' expr

lambda_expr  ::= 'λ' IDENT+ '→' expr

match_expr   ::= 'match' expr '{' match_arm* '}'

match_arm    ::= pattern '→' expr ','?

if_expr      ::= 'if' expr 'then' expr 'else' expr

handle_expr  ::= 'handle' '{' handle_arm* '}' 'in' expr

handle_arm   ::= app_type '→' expr ','?

spawn_expr   ::= 'spawn' '(' IDENT ( ',' expr )* ')'
```

`spawn`'s first argument is an actor name, resolved in the actor namespace;
actor names are not first-class values.

## Infix Expressions (precedence climbing)

Precedence from lowest to highest:

| Prec | Operators                  | Assoc | Description          |
|------|----------------------------|-------|----------------------|
| 1    | `\|\|`                     | left  | logical or           |
| 2    | `&&`                       | left  | logical and          |
| 3    | `==` `!=` `<` `>` `<=` `>=` | none  | relational           |
| 4    | `+` `-`                    | left  | additive             |
| 5    | `*` `/`                    | left  | multiplicative       |
| 6    | application                | left  | function application |
| 7    | `.`                        | left  | field access         |

The logical operators `&&` and `||` (canonical `∧` `∨`) are left-associative
and bind looser than the relational tier, so `a == b && c == d` groups as
`(a == b) && (c == d)`.

The relational operators form a single non-associative tier: a chain such
as `a == b == c` or `a < b == c` is a parse error (`P0005`). Compare
chained results explicitly with parentheses: `(a == b) == c`.

```
infix_expr   ::= prefix_expr ( bin_op prefix_expr )*

bin_op       ::= '||' | '&&'
               | '==' | '!=' | '<' | '>' | '<=' | '>='
               | '+' | '-' | '*' | '/'

prefix_expr  ::= app_expr

app_expr     ::= postfix_expr+

postfix_expr ::= atom_expr ( '.' IDENT )*

atom_expr    ::= IDENT
               | INT
               | FLOAT
               | STR
               | '(' expr ')'
               | tuple_lit
               | list_lit
               | record_lit
               | path

tuple_lit    ::= '(' expr ',' expr ( ',' expr )* ','? ')'

list_lit     ::= '[' ( expr ( ',' expr )* ','? )? ']'

record_lit   ::= '{' ( record_field ( ',' record_field )* ','? )? '}'

record_field ::= field_name ':' expr

field_name   ::= IDENT | keyword
```

A `field_name` may be a keyword spelling (e.g. `actor: Planner` in a supervisor
child spec); the `name :` shape leaves no ambiguity.

## Patterns

```
pattern      ::= constructor_pat
               | tuple_pat
               | literal_pat
               | wildcard_pat
               | bind_pat

constructor_pat ::= IDENT ( '(' pattern ( ',' pattern )* ','? ')' )?

tuple_pat    ::= '(' pattern ',' pattern ( ',' pattern )* ','? ')'

literal_pat  ::= INT | FLOAT | STR

wildcard_pat ::= '_'

bind_pat     ::= IDENT
```

Note: `constructor_pat` and `bind_pat` are disambiguated by naming convention
— PascalCase is a constructor, snake_case is a binding. The lexer enforces
canonical naming.

## Type Expressions

```
type_expr    ::= fn_type

fn_type      ::= app_type ( '→' app_type effect_ann? )*

app_type     ::= atom_type ( '<' type_args '>' )?

type_args    ::= type_expr ( ',' type_expr )* ','?

atom_type    ::= IDENT
               | '(' type_expr ')'
               | tuple_type

tuple_type   ::= '(' type_expr ',' type_expr ( ',' type_expr )* ','? ')'
```

## Lexical Grammar (reference)

See `hird-lex` crate for the authoritative implementation. Summary:

- **Keywords**: `let`, `fn`, `match`, `type`, `actor`, `supervisor`, `effect`,
  `tool`, `handle`, `spawn`, `send`, `request`, `use`, `module`, `pub`,
  `extern`, `if`, `then`, `else`, `in`
- **Identifiers**: ASCII alphanumeric + underscore, canonical naming enforced
  (snake_case for values, PascalCase for types)
- **Literals**: integers, floats, double-quoted strings with escape sequences
- **Operators**: `+` `-` `*` `/` `<` `>` `<=` `>=` `==` `!=` `=` `→` `⇒`
  `λ` `|` `!` `.` `:` `::`
- **Delimiters**: `(` `)` `{` `}` `[` `]` `,` `;`
- **Comments**: `// line` and `/* block */` (nestable)
- **Unicode normalisation**: `->` to `→`, `=>` to `⇒`, `\` to `λ`

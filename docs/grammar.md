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
               | type_alias
               | actor_decl
               | supervisor_decl
               | effect_decl
               | tool_decl
               | extern_decl
```

## Use Declarations

```
use_decl     ::= 'use' path ( use_group | 'as' IDENT )?

path         ::= IDENT ( '.' IDENT )*

use_group    ::= '.' '{' IDENT ( ',' IDENT )* ','? '}'
```

`use Mod` binds the trailing path segment as a qualifier for
`Mod.member` access; `use Mod as M` binds `M` instead. `use Mod.{a, b}`
binds the listed members unqualified — and only those; it does not also
bind the qualifier. A selective import cannot also be aliased. `as` is
a contextual keyword (an ordinary identifier elsewhere), not a reserved
word.

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
type_decl    ::= ( 'pub' 'opaque'? )? 'type' IDENT type_params? '=' constructors

type_params  ::= '<' IDENT ( ',' IDENT )* ','? '>'

constructors ::= '|'? constructor ( '|' constructor )*

constructor  ::= IDENT ( '(' field_list ')' )?

field_list   ::= type_expr ( ',' type_expr )* ','?
```

`opaque` is legal only in the `pub opaque type` form: the type's name is
exported while its constructors stay module-private.

## Type Aliases

```
type_alias   ::= 'pub'? 'type' 'alias' IDENT type_params? '=' type_expr
```

`alias` is contextual: an identifier in the slot after `type` selects the
alias form (a type name is `PascalCase`, so the two cannot collide). An
alias names a type expression and has no identity of its own; every use
expands to the right-hand side. `opaque` on an alias is a parse error
(P0007): there are no constructors to hide.

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

## Supervisor Declarations

```
supervisor_decl  ::= visibility? 'supervisor' IDENT '{' supervisor_body? '}'

supervisor_body  ::= supervisor_field ( ',' supervisor_field )* ','?

supervisor_field ::= IDENT ':' expr
```

## Effect Declarations

```
effect_decl  ::= visibility? 'effect' IDENT type_params?
```

## Tool Declarations

```
tool_decl    ::= visibility? 'tool' IDENT type_params? ':' app_type '→'
                  type_expr effect_ann?
```

The argument position takes an `app_type` (in practice a record type or
a named type), not a full function type; the optional trailing row
unions into the generated function's row.

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
               | install_expr
               | spawn_expr
               | supervise_expr
               | stand_expr
               | clock_expr
               | self_expr
               | child_expr
               | send_expr
               | request_expr
               | schedule_expr
               | reply_expr
               | crash_expr
               | infix_expr

let_expr     ::= 'let' IDENT ( ':' type_expr )? '=' expr 'in' expr

lambda_expr  ::= 'λ' IDENT+ '→' expr

match_expr   ::= 'match' expr '{' ( match_arm ( ',' match_arm )* ','? )? '}'

match_arm    ::= pattern '→' expr

if_expr      ::= 'if' expr 'then' expr 'else' expr

handle_expr  ::= 'handle' handler_arms 'in' expr

install_expr ::= 'install' handler_arms 'in' expr

handler_arms ::= '{' ( handle_arm ( ',' handle_arm )* ','? )? '}'

handle_arm   ::= app_type '→' expr

spawn_expr   ::= 'spawn' '(' IDENT ( ',' expr )* ','? ')'

supervise_expr ::= 'supervise' '(' IDENT ')'

stand_expr   ::= 'stand' '(' ')'

clock_expr   ::= 'clock' '(' ')'

self_expr    ::= 'self' '(' ')'

child_expr   ::= 'child' '(' IDENT ',' IDENT ')'

send_expr    ::= 'send' '(' expr ',' expr ')'

request_expr ::= 'request' '(' expr ',' expr ( ',' expr )? ')'

schedule_expr ::= 'schedule' '(' expr ',' expr ',' expr ',' expr ')'

reply_expr   ::= 'reply' '(' expr ',' expr ')'

crash_expr   ::= ( 'crash' | 'panic' ) '!' '(' expr ')'
```

`match`, `handle`, and `install` arms are comma-separated (trailing
comma optional); a missing comma between arms is a parse error.

`spawn`'s first argument is an actor name, resolved in the actor
namespace; `supervise`'s argument and `child`'s first argument are
supervisor names, resolved in the supervisor namespace; `child`'s
second argument is one of that supervisor's declared child ids. None of
these are expressions — actor and supervisor names are not first-class
values.

`send` and `reply` are keyword forms taking exactly two expression
arguments; `request` takes two, plus an optional third (the timeout in
milliseconds); `schedule` takes four (clock, destination, message, delay
in milliseconds). `stand`, `clock`, and `self` take none. `clock` is
contextual: it is the form only as `clock()`, and an ordinary identifier
elsewhere (so `clock: Clock` is a legal parameter).

`crash!` (and its alias `panic!`) is the divergent primitive: it takes a
single `String` message, never returns, and propagates as a process exit to
the supervisor. The `!` is a required part of the form.

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
               | unit_lit
               | '(' expr ')'
               | tuple_lit
               | list_lit
               | record_lit

unit_lit     ::= '(' ')'

tuple_lit    ::= '(' expr ',' expr ( ',' expr )* ','? ')'

list_lit     ::= '[' ( expr ( ',' expr )* ','? )? ']'

record_lit   ::= '{' ( record_field ( ',' record_field )* )? ( ',' record_base )? ','? '}'

record_field ::= field_name ':' expr

record_base  ::= '..' expr

field_name   ::= IDENT | keyword
```

A `field_name` may be a keyword spelling (e.g. `actor: Planner` in a supervisor
child spec); the `name :` shape leaves no ambiguity.

A `..base` tail makes the literal an update: the listed fields come from
the literal and every other field from `base`, whose record type the result
keeps. The base is written once, last (P0008 otherwise); a literal with a
base and no fields is rejected by the checker (C0060).

A `{` never begins an application argument: record-literal arguments
must be parenthesised (`f({ x: 1 })`, not `f { x: 1 }`). Qualified
access (`Mod.member`) has no dedicated production — it parses as field
access on a `PascalCase` receiver and is resolved in the checker.

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
               | record_type
               | unit_type
               | '(' type_expr ')'
               | tuple_type

record_type  ::= '{' ( record_type_field ( ',' record_type_field )* ','? )? '}'

record_type_field ::= field_name ':' type_expr

unit_type    ::= '(' ')'

tuple_type   ::= '(' type_expr ',' type_expr ( ',' type_expr )* ','? ')'
```

A `{` where a type is expected always begins a record type; braces
never delimit anything else in type position.

## Lexical Grammar (reference)

See `hird-lex` crate for the authoritative implementation. Summary:

- **Keywords**: `let`, `fn`, `match`, `type`, `actor`, `supervisor`, `effect`,
  `tool`, `handle`, `install`, `spawn`, `supervise`, `stand`, `self`,
  `child`, `send`, `request`, `schedule`, `reply`, `crash`, `panic`, `use`,
  `module`, `pub`, `opaque`, `extern`, `if`, `then`, `else`, `in`. (`as`,
  `alias`, and `clock` are contextual, not reserved.)
- **Identifiers**: ASCII alphanumeric + underscore, canonical naming enforced
  (snake_case for values, PascalCase for types)
- **Literals**: integers, floats, double-quoted strings with escape sequences
- **Operators**: `+` `-` `*` `/` `<` `>` `<=` `>=` `==` `!=` `=` `→` `⇒`
  `λ` `|` `!` `.` `..` `:` `::`
- **Delimiters**: `(` `)` `{` `}` `[` `]` `,` `;`
- **Comments**: `// line` and `/* block */` (nestable)
- **Unicode normalisation**: `->` to `→`, `=>` to `⇒`, `\` to `λ`

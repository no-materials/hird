# The effect graph

`hird emit-effect-graph <input>` projects a program's actors, supervisors,
tools, and functions with their effect rows: what every part of the
program may reach.
It is the surface teams commit as an approved baseline and diff against, so
its shape and its identity rules are stated here.

## The JSON document

`--json` emits one document for a single file and for a directory alike:
the program's module graphs keyed by module name.

```json
{
  "schema_version": 1,
  "modules": {
    "AgentFleet": { "schema_version": 1, "module": "AgentFleet", "actors": [], "supervisors": [], "tools": [] },
    "Errands":    { "schema_version": 1, "module": "Errands",    "actors": [], "supervisors": [], "tools": [] }
  }
}
```

Keys sort by module name, so emission order never depends on file order.
Each module graph carries its own `schema_version` and `module` so it
stays self-describing when consumed on its own (the MCP server serves
module graphs directly).

Within a module:

- `actors`: `name`, `line`, `state`, `message` (`name`, `constructors`
  with `fields`), `init` (`params`, `effects`), `handlers` (`message`,
  `effects`), and the declared summary `effects`.
- `supervisors`: `name`, `line`, `strategy`, `intensity`, `period`,
  `children` (`id`, `actor`, `restart`), and the derived `effects`.
- `tools`: `name`, `line`, `params`, `input`, `output`, `effects`.
- `functions`: `name`, `line`, `params` (`name`, `type`), `result`, and the
  declared `effects`. Plain functions, so a reviewer reads off the graph
  that a function is honestly `! {Tool<X>}` and nothing more.

Types and effect rows appear twice: `display` is the canonical surface
syntax (`{Tool<ReadRepo>, Send<Status>}`), `structure` the same value as
data. Both render one value; a diff may compare either.

`schema_version` is `1`. The schema evolves additively: fields are added,
never renamed or removed, without a bump.

## Identity contract

A baseline diff must flag real changes and stay quiet through unrelated
edits, so every field is one of two kinds.

| Kind | Fields | Rule |
|---|---|---|
| Incidental | every `line` | Locates the declaration in its file. Ignored by diffs. |
| Incidental | order of `actors`, `supervisors`, `tools`, `functions`, `handlers`, `constructors` | Source order. Key entries by `name` (handlers by `message`). |
| Identity | everything else | Module names; actor, supervisor, tool, function, message-type, and constructor names; every type and effect row; tool type parameters; supervisor strategy, intensity, period, and children. |

Children keep their order as identity: start order drives `rest_for_one`.

## The text form

Without `--json` the same projection prints as indented text, one block
per module. Declarations are located by `file:line`, where `file` is the
module's own file name (`agent_fleet.hird:12`), never a checkout path, so
the text is stable across machines. The text is for reading; commit and
diff the JSON.

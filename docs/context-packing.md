# Token-budget-aware context packing

`get_context_for_symbol` is how an agent should consume a Hirð codebase:
ask the compiler for one symbol, name a token budget, and get a
prompt-ready summary that fits it, plus a manifest of what did not. The
agent never reads source it cannot afford, and it always knows what it
did not see.

The summary is assembled from sections in a fixed order — signature,
effects, doc comment, callers, callees. Each section is kept if the
summary still fits the budget with it appended and dropped (and listed
under `omitted`) otherwise. The signature is the one exception: it is
never dropped, only truncated into the budget. Tokens are estimated at
about four characters each; the estimate is a heuristic, not a tokenizer.

## The same actor at two budgets

Both transcripts below are real `tools/call` exchanges with `hird-mcp`
over stdio, against `demo/agent_planner.hird`. Only the tool's
`structuredContent` is shown; the `content` text block carries the same
document.

Request, 50 tokens:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
  "name":"get_context_for_symbol",
  "arguments":{"file":"demo/agent_planner.hird","name":"Planner","budget":50}}}
```

Response:

```json
{
  "file": "demo/agent_planner.hird",
  "symbol": "Planner",
  "kind": "actor",
  "budget": 50,
  "approx_tokens": 39,
  "summary": "actor Planner — state { repos: Int, tickets: Int }, message PlannerMsg = PlanRepo(Path) | GetStatus(ReplyTo<PlannerStatus>) | Shutdown\ncallers: PlannerSup",
  "omitted": ["effects", "callees"]
}
```

The signature alone is most of the budget. The effect row did not fit on
top of it and was dropped; the callers line, being shorter, did. Dropping
is per section, not a prefix cut, so a later short section can survive an
earlier long one. The agent sees exactly which two sections it is missing
and can ask for them directly (`explain_effect_row`, `lookup_definition`)
or raise the budget.

Request, 400 tokens:

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
  "name":"get_context_for_symbol",
  "arguments":{"file":"demo/agent_planner.hird","name":"Planner","budget":400}}}
```

Response:

```json
{
  "file": "demo/agent_planner.hird",
  "symbol": "Planner",
  "kind": "actor",
  "budget": 400,
  "approx_tokens": 70,
  "summary": "actor Planner — state { repos: Int, tickets: Int }, message PlannerMsg = PlanRepo(Path) | GetStatus(ReplyTo<PlannerStatus>) | Shutdown\neffects: {Send<PlannerStatus>, Tool<CreateTicket>, Tool<Log>, Tool<ReadRepo>}\ncallers: PlannerSup\ncallees: analyze, file_tickets, log, read_repo",
  "omitted": []
}
```

Everything fits with room to spare: the mailbox, the actor's effect
summary (what it may reach — three tools and one reply channel), who
starts it, and what it calls. Seventy tokens is the whole actor, at the
level an agent needs before deciding whether to open the source.

## Sizing before asking

`get_context_budget` reports a module's approximate token cost by category
(types, functions, effects) without naming symbols, so an agent can decide
between packing one symbol tightly and reading a module outline with
`list_definitions`, which carries a per-symbol cost. A budget at or above
a symbol's signature cost keeps the signature whole; below it, the
signature is truncated and nothing else is included.

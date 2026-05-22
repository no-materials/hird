---
id: hir-4g3y
status: open
deps: [hir-t1cj]
links: [hir-x6cx]
created: 2026-05-22T21:39:26Z
type: task
priority: 1
assignee: nomaterials
parent: hir-jt39
tags: [phase-6, tools]
---
# Tool declarations and invocation records

Implement tool declarations as a special class of effect and the compiler-
generated invocation record types.

**Tool declaration syntax**:
```
tool ReadRepo : { path: Path } -> RepoState
tool CreateTicket : { title: String, body: String } -> TicketId
tool LLMCall<T> : { prompt: Prompt, schema: Schema<T> } -> T ! {Exn ParseError}
tool HttpGet : { url: Url, headers: Headers } -> HttpResponse ! {Exn HttpError}
```

A tool declaration creates:
1. An effect: Tool<ReadRepo>, Tool<CreateTicket>, etc.
2. A function: read_repo(args) -> RepoState ! {Tool<ReadRepo>}.
3. An invocation record type (compiler-generated):
   ```
   type ReadRepoInvocation = {
     tool: "ReadRepo",
     args: { path: Path },
     result: RepoState,
     timestamp: Timestamp,
     caller: CallerId,
   }
   ```

**Integration with effect system**:
- Tool<X> is a valid effect in any effect row.
- Tool effects compose with other effects normally.
- Tool effects are handleable via DI-style handlers (from Phase 5).

**Standard library tool declarations** (in a prelude or standard module):
- llm_call<T>: schema-typed LLM invocation (resolves OD2).
- http_get, http_post: HTTP operations.
- read_file, write_file: filesystem operations.
- shell: shell command execution.

Each standard tool has a proper invocation record type.

This ticket resolves **OD2 (LLM call typing)**: confirm schema-typed approach.

## Acceptance Criteria

- tool declaration syntax parsed, type-checked, and registered.
- Each tool creates an effect, a function, and an invocation record type.
- Tool effects integrate with the general effect row system.
- Standard library tools declared: llm_call, http_get, http_post, read_file,
  write_file, shell.
- Invocation record types are compiler-generated with correct fields.
- OD2 documented in DECISIONS.md.
- Snapshot tests: tool declaration, tool call in effect row, tool invocation
  record structure, standard library tool types.
- At least 8 snapshot tests.


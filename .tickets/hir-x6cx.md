---
id: hir-x6cx
status: open
deps: []
links: [hir-4g3y]
created: 2026-05-22T21:43:02Z
type: task
priority: 1
assignee: nomaterials
tags: [decision, design, tools, llm]
---
# OD2: LLM call typing

Resolve how LLM calls are typed in Hirð.

**Strong lean**: schema-typed with automatic structured output.
  llm_call<T>(prompt: Prompt, schema: Schema<T>) -> T ! {Tool<LLM>, Exn ParseError}

The schema parameter tells the LLM what structured output to produce. The
compiler knows the return type T. If the LLM's output doesn't parse to T,
Exn ParseError is raised.

**Alternatives**:
1. Raw text: llm_call(prompt) -> String ! {Tool<LLM>} — caller parses manually.
   Too untyped; loses the safety story.
2. Opaque response: llm_call(prompt) -> LLMResponse ! {Tool<LLM>} — accessor
   methods on the response. Better than raw text but still untyped at the
   extraction point.
3. Probabilistic: llm_call(prompt) -> Dist<T> ! {Tool<LLM>} — captures
   uncertainty. Interesting but significantly more complex; deferred.

**Decision point**: Phase 6 implementation.

## Acceptance Criteria

- Decision documented in DECISIONS.md.
- Tool declaration for llm_call reflects the chosen typing.
- At least one example in phrasebook.md showing LLM call usage.


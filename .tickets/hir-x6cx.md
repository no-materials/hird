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


## Notes

**2026-07-01T13:47:08Z**

OD2 (LLM call typing) resolved: schema-typed, confirmed. Documented in DECISIONS.md
as ADR-015 clause 3 and removed from the open-decision-slots table.

Locked shape:
  llm_call<t> : { prompt: Prompt, schema: Schema<t> } → t ! {Exn ParseError}
The caller supplies Schema<t>; the result type t is tied to it by ordinary
unification; a non-conforming result raises Exn ParseError. Raw-text,
opaque-response, and Dist<t> alternatives rejected for v0.1.

Remaining ACs (the llm_call tool declaration reflecting this typing, and a
phrasebook.md example of LLM call usage) are implemented as part of the tool
declarations work — left open here until that lands.

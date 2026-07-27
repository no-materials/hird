# Hirð Runtime

Erlang support library for compiled Hirð programs.

This directory contains the Erlang modules that compiled Hirð code depends
on at runtime. It is not a Rust crate — it ships alongside the compiled
`.erl` output and is loaded by the BEAM VM. The runtime is a dependency,
not a framework: each module is small, and generated code touches only the
dispatcher.

## Modules

- `hird_tool_dispatch.erl` — the tool effect dispatcher. Every generated
  tool call site invokes `call(ToolName, Caller, Handlers, Args)`: the
  handler is `{tool, ToolName}` in the threaded map, falling back to the
  default registry, and a miss in both crashes with
  `{unhandled_tool, ToolName}`. Every invocation — mocked or real — is
  captured as an invocation record and sent to the audit sink.
- `hird_audit.erl` — the audit log sink: a `gen_server` writing canonical
  JSON lines (the wire format of `docs/tool-effects.md`) to stdout or an
  append-only file, in arrival order. Encoding is type-directed against
  the signature table generated base modules expose as `hird_tools@/0`,
  registered via `register_tools/1`.
- `hird_handlers.erl` — the process-independent default-handler registry:
  `install_handler/2`, `lookup_handler/1`, `with_handlers/2`. Handler maps
  never cross the spawn boundary, so this registry is how deployments and
  test harnesses supply handlers (and mocks) to spawned actors.
- `hird_types.erl` — the canonical wire encoder for invocation records; it
  reproduces the golden files under `conformance/v1` byte for byte.
- `hird_sup_util.erl` — supervisor utilities: `child_pid/2` looks up an
  unregistered supervised child's pid. Generated supervisor modules carry
  their child specs inline, so nothing more is needed here.

## Tests

```sh
./test.sh
```

compiles everything with `erlc -Werror` and runs the eunit suites under
`test/`, including byte-exact reproduction of the `conformance/v1` audit
log goldens.

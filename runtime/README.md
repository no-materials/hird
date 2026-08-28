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
  `{unhandled_tool, ToolName}`. While a replay cursor is running, the
  dispatcher instead consults it for every call — no handler in the
  program can shadow the log. Every invocation — mocked, real, or
  replayed — is captured as an invocation record and sent to the audit
  sink. A handler signals a domain failure by throwing
  `{hird_exn, Error}`: the dispatcher records an `{err, Error}` result
  and rethrows; any other exception is a crash, propagated untouched and
  unrecorded.
- `hird_audit.erl` — the audit log sink: a `gen_server` writing canonical
  JSON lines (the wire format of `docs/tool-effects.md`) to stdout or an
  append-only file, in arrival order. Encoding is type-directed against
  the signature table generated base modules expose as `hird_tools@/0`,
  registered via `register_tools/1`.
- `hird_handlers.erl` — the process-independent default-handler registry:
  `install_handler/2`, `lookup_handler/1`, `with_handlers/2`. Handler maps
  never cross the spawn boundary, so this registry is how deployments and
  test harnesses supply handlers (and mocks) to spawned actors.
- `hird_types.erl` — the canonical wire encoder and decoder for invocation
  records; the encoder reproduces the golden files under `conformance/v1`
  byte for byte, and the decoder round-trips them.
- `hird_replay.erl` — the replay cursor: a `gen_server` holding a recorded
  audit log, decoded type-directedly at startup and matched
  strict-sequentially against the program's tool dispatches. Any mismatch
  is a structured divergence the dispatcher raises as a crash;
  `finish/0` reports a log the run did not fully consume.
- `hird_sup_util.erl` — supervisor utilities: `child_pid/2` looks up an
  unregistered supervised child's pid. Generated supervisor modules carry
  their child specs inline, so nothing more is needed here.
- `hird_stand.erl` — standing mode: `await/0` blocks the caller until a
  stop trigger fires — SIGTERM where the platform has it, or end of file
  on stdin when the launcher passed `-hird_stop stdin` (`hird run` owns
  that pipe and closes it on Ctrl-C or termination, on every platform) —
  then shuts down every supervisor the caller started (its linked
  `supervisor` children, in reverse start order, by the OTP
  parent-shutdown protocol) and returns, so the boot module's audit sync
  runs after the trees are gone. It replaces OTP's default signal handler
  for the node's lifetime.
- `hird_clock.erl` — the clock capability: `real/0` is what `clock()`
  lowers to, and `schedule/4` is `schedule(clock, pid, msg, delay_ms)`:
  `erlang:send_after` into the destination's cast path. The clock value
  carries its implementation; a scheduled message is not cancellable,
  and one aimed at an exited pid is dropped.

## Tests

```sh
./test.sh
```

compiles everything with `erlc -Werror` and runs the eunit suites under
`test/`, including byte-exact reproduction of the `conformance/v1` audit
log goldens.

CI runs this script, and then the workspace test suite with `erlc` on
`PATH`, in the one job that installs Erlang/OTP. That job sets
`HIRD_REQUIRE_BEAM`, which turns the "skipping: erlc not found on PATH"
path of the Rust tests into a failure, so a broken toolchain install
cannot pass as a green run.

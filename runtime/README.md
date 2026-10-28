# Hirð Runtime

Erlang support library for compiled Hirð programs.

This directory contains the Erlang modules that compiled Hirð code depends
on at runtime. It is not a Rust crate — it ships alongside the compiled
`.erl` output and is loaded by the BEAM VM. The runtime is a dependency,
not a framework: each module is small, and generated code touches only the
dispatcher.

## Modules

- `hird_tool_dispatch.erl` — the tool effect dispatcher. Every generated
  tool call site invokes `call(ToolName, TypeArgs, Caller, Handlers,
  Args)`, `TypeArgs` being the wire shapes the site fixes for a generic
  tool's type parameters (`[]` otherwise): the handler is
  `{tool, ToolName}` in the threaded map, falling back to the default
  registry, and a miss in both crashes with `{unhandled_tool, ToolName}`.
  While a replay cursor is running, the dispatcher instead consults it for
  every call — no handler in the program can shadow the log. Every
  invocation — mocked, real, or replayed — is captured as an invocation
  record and sent to the audit sink, its caller id qualified with the
  calling process's identity (`Caller@Process`). A handler signals a
  domain failure by throwing `{hird_exn, Error}`: the dispatcher records
  an `{err, Error}` result and rethrows; any other exception is a crash,
  propagated untouched and unrecorded.
- `hird_audit.erl` — the audit log sink: a `gen_server` writing canonical
  JSON lines (the wire format of `docs/tool-effects.md`) to stdout or an
  append-only file, in arrival order. Encoding is type-directed against
  the signature tables generated base modules expose as `hird_tools@/0`
  (every module declaring a tool or an ADT has one), registered via
  `register_tools/1`, and runs in the calling process: the sink publishes
  the merged table as a persistent term and only appends finished lines,
  writing a batch once 64 KiB are buffered or 10 ms after its first line,
  and on `sync/0`. `log/1` casts while the sink has under 8 MiB of lines
  queued and waits in a call past that, so a burst slows its producers
  instead of growing the sink's mailbox without bound.
- `hird_handlers.erl` — the process-independent default-handler registry:
  `install_handler/2`, `lookup_handler/1`, `with_handlers/2`. Handler maps
  never cross the spawn boundary, so this registry is how deployments and
  test harnesses supply handlers (and mocks) to spawned actors.
- `hird_types.erl` — the canonical wire encoder and decoder for invocation
  records, a generic tool's shapes instantiated at each record's type
  arguments and the predeclared `Option` and `Next` built in; the encoder
  reproduces the golden files under `conformance/v1` byte for byte, and
  the decoder round-trips them.
- `hird_replay.erl` — the replay cursor: a `gen_server` holding a recorded
  audit log, decoded type-directedly at startup — a generic tool's
  records when their call is offered, at its type arguments — and split
  into one stream per process, the identity each record's caller names.
  `offer/4` matches a process's dispatch strict-sequentially against its
  own stream; streams interleave freely. Any mismatch is a structured
  divergence the dispatcher raises as a crash; `finish/0` reports a log
  the run did not fully consume.
- `hird_process.erl` — process identity: the run-stable name the
  dispatcher records and replay matches by — `main`, `Sup/child` for a
  supervised child, `Spawner/Actor#N` for a spawned actor. Generated code
  names each process once, before its first tool call (`name/1`: the
  boot module names `main`, an actor's `init` the identity its
  `start_link` was given), and `spawned/1` names a spawn's child.
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
  runs after the trees are gone. A supervisor that exits on its own first —
  its restart budget exhausted — ends the wait as well: the other trees
  are stopped the same way and `await` raises, so the program exits 1.
  It replaces OTP's default signal handler for the node's lifetime.
- `hird_clock.erl` — the clock capability: `real/0` is what `clock()`
  lowers to, and `schedule/4` is `schedule(clock, pid, msg, delay_ms)`:
  `erlang:send_after` into the destination's cast path. The clock value
  carries its implementation; a scheduled message is not cancellable,
  and one aimed at an exited pid is dropped.
- `hird_build.erl` — the build driver: compiles the modules a build did
  not find in the build cache inside the emulator that goes on to run
  them, so `hird run` boots one emulator (`run/0` then calls
  `hird_boot:run/0`; `build/0`, for `hird build`, halts instead). The
  launcher passes the output directory, the modules, and the cache
  directory with one entry key per module as init arguments. Modules
  compile in parallel, one worker per scheduler; each is written to the
  output directory and stored in the cache aside-then-rename, and the
  compiler's reports go to standard error in module order, never to the
  program's standard output. Generated programs do not call it.

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

## Benchmarks

```sh
./bench/bench.sh <scenario> [--quick]
```

compiles the runtime and `bench/` into `bench/_build`, then runs one
scenario, or `all`, printing a table per scenario with ops/s, p50, p99
and max latency, and memory for each case:

- `audit` — dispatch throughput through the audit sink: 1 and 16
  producers, 16 B, 4 KiB and 64 KiB payloads, file and stdout sinks.
- `sync` — how long `sync/0` waits behind the backlog a burst leaves.
- `mailbox` — the sink's queue and memory over time under a producer
  paced at 10k and at 200k calls per second.
- `replay` — cursor load time and memory for 50k, 200k and 1M record
  logs, then offer throughput.
- `child` — a `child(Sup, id)` lookup and a send, from 1 and 16 callers.
- `restart` — lookup latency while a sibling restarts with a 1 s init.
- `install` — `install_handler/2` replacing a handler, against 0 to
  100k processes.

The scenarios call the runtime as generated code does
(`hird_tool_dispatch:call/5`, `hird_sup_util:child_pid/2` on a
registered supervisor), so a change behind those calls shows in the
numbers without touching the bench. The report goes to stdout and the
emulator's own standard output to `/dev/null`, so the stdout sink
figures cover the Erlang io path, not a terminal. Throughput loops time
one operation in 16, since a clock read is a syscall on some clock
sources (`restart`, `install` and the `sync` wait time every one); the
header prints what timing adds to each latency on the machine at hand.
Replay logs are written once per size under `bench/_build` and reused:
the 1M log is 140 MB, and a full `replay` run takes minutes.

`--quick` shrinks every size for a smoke run; CI runs `all --quick` on
Linux after the eunit suites, so the harness keeps up with the runtime.

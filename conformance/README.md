# Audit-log conformance suite

Golden files for the tool-invocation wire format, one directory per
`schema_version`. The format is specified normatively in
`docs/tool-effects.md`; the reference implementation is the `wire` module
of `hird-check`, whose test suite asserts that encoding the corresponding
records reproduces these files **byte for byte** and that decoding them
against the tools' signatures round-trips.

Any other producer or consumer of the format must reproduce these bytes
exactly. The files are the contract; the Rust implementation is the oracle
that generated them. The Erlang runtime is the second implementation held
to it: `runtime/hird_types.erl` reproduces every file here byte for byte
and round-trips it back through its decoder, asserted by the runtime's
eunit suite.

Which changes may reuse this directory and which need a new one is the
wire-format stability policy in `docs/audit-evidence.md`.

- `v1/read_repo_ok.json` — a successful invocation, with `meta`.
- `v1/create_ticket_ok.json` — an ADT result, no `meta`.
- `v1/http_get_err.json` — a failed invocation (`err`-tagged result).
- `v1/planner_log.jsonl` — the three records as one JSON-lines log.

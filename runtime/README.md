# Hirð Runtime

Erlang support library for compiled Hirð programs.

This directory contains the Erlang modules that compiled Hirð code depends on
at runtime. It is not a Rust crate — it ships alongside the compiled `.erl`
output and is loaded by the BEAM VM.

Planned contents (not yet implemented):

- `hird_rt.erl` — process startup, capability wiring, effect dispatch
- `hird_sup.erl` — thin wrapper over OTP supervisor behaviors
- `hird_msg.erl` — typed message envelope encoding/decoding

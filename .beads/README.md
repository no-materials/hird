# Hirð issues

This repository uses [Beads](https://github.com/gastownhall/beads) (`bd`) for
in-repo issue tracking. New issues use the `hir-*` prefix.

## Storage and synchronization

The authoritative issue database is a Dolt history stored in this repository's
`refs/dolt/data` ref. The local database under `.beads/embeddeddolt/` is
machine-local and ignored by Git. JSONL exports are for interchange only; they
are not the synchronization or backup mechanism.

After a fresh clone:

```sh
# On Unix, keep the local database private to your user.
chmod 700 .beads
bd bootstrap
```

For each work session:

```sh
bd prime
bd dolt pull
bd ready

# Inspect or change work.
bd show <issue-id>
bd create "Concise outcome" --description "Context and acceptance criteria"
bd update <issue-id> --claim
bd close <issue-id>

bd dolt push
```

Beads changes and source changes are published independently. `git push`
publishes source branches; `bd dolt push` publishes issue history. Automatic
Beads pushes are intentionally disabled.

## History

The tracker was migrated from the previous plain-markdown system in
`.tickets/`; migrated issues keep their original ids (`hir-*`, plus a handful
of `ha-*`/`hc-*`/`hi-*`/`hl-*` ids from per-crate prefixes).

Use labels for stable areas such as `parser`, `types`, `effects`, `actors`,
`codegen`, `supervision`, and `tooling`. Use epics for concrete multi-issue
outcomes rather than permanent subsystem buckets.

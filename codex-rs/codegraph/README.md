# codegraph data layer (vendored snapshot)

These 7 crates are a **snapshot copy** of the codegraph data layer, vendored
into `codex-rs` so `codex-rs` stays self-contained (no path dependency on the
standalone `codegraph-rs` repo):

- `codegraph-core` — semgraph types (Symbol, FlowResult, …)
- `codegraph-source` — source file / git access
- `codegraph-extract` — tree-sitter extraction (`all-langs`, no `binary`)
- `codegraph-graph` — graph storage (**sqlite only**; no redis/lmdb/postgres/mysql)
- `codegraph-api` — `Session` lifecycle + `GraphApi` query surface
- `codegraph-context` — Jev rerank / context building
- `codegraph-docs` — docs graph

Source of truth: `../../codegraph-rs/crates/<crate>` (monorepo sibling).
Sync is manual: re-copy, re-apply the local patches listed below, then run
`cargo check -p codegraph-api`.

## Local patches vs. upstream

- `codegraph-core`: `graphql` feature + `async-graphql` derives removed.
- `codegraph-extract`: `binary` feature kept as a no-op (code is `cfg`'d
  out); `codegraph-binary` dep dropped.
- `codegraph-graph`: direct `sqlx` 0.9 dep (needs `sqlite-load-extension`
  for sqlite-vss, which the shared workspace `sqlx` doesn't enable); sqlx 0.9
  requires `unsafe { .extension(...) }`.
- `codegraph-api`: `tools` module (MCP sandbox/diff/simulate) dropped;
  `codegraph-graph` forced to `sqlite` only, `codegraph-extract` to
  `default-features = false, features = ["all-langs"]`; `codegraph-sboxes`
  dep dropped.

## Usage in codex-rs

- Host indexing (`codex codegraph index`, TUI `/index`) runs in the host
  process; the agent only calls `codegraph.call` verbs
  `begin` → `search` → `end` via `codegraph_api::Session`.
- Cargo feature `codegraph` (in `codex-core` / `codex-cli`) gates the whole
  surface; `CodegraphConfig.enabled` gates runtime registration.
- Future cloud mode (`mode = "cloud"`) is a config marker only — today
  everything routes through `Session` (dsn-based, `sqlite://`).

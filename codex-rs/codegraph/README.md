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
  process via `codex_codegraph_server::CodegraphService::host_index`.
- **Agent tool surface: built-in compatibility MCP server** (like
  `codex_apps`). When `[codegraph] enabled`, `McpManager` registers a
  `codegraph` MCP server that is served **in-process** on a duplex stream
  (`servers/codegraph-server`, `codex-codegraph-server::serve_on`) — no
  subprocess is spawned. Tools are flat MCP functions with rich
  descriptions, so tool search and Jev see full per-tool signal.
- Cargo feature `codegraph` (in `codex-cli`) gates the surface;
  `CodegraphConfig.enabled` gates registration.

## Tool surface coverage

The embedded server exposes the query surface backed by the vendored
crates: symbol/call-graph search, resolve, callers/callees, context, flow,
`trace_full_flow`, impact, references, `search_flow`, files, stats,
`mermaid` diagrams, session lifecycle (`begin`/`end`/`status`), and the
docgraph verbs (`doc_search`, `doc_list`, `doc_stats`, `doc_hydrate`,
`doc_struct`, `mine_patterns`, `list_patterns`).

Some upstream MCP-server tools are **not available here** because they need
crates that are not vendored:

- `sandbox` / `diff_simulate` / `origin_simulate` — require the sboxes/Cranelift
  JIT sandbox (`codegraph-sboxes` dep dropped, see patches above).
- `graphbin` — binary graph needs the `binary` feature (cfg'd out).
- `config_effects` writes and `usage_report` — upstream-server-side services.

Use the standalone `codegraph-rs` MCP server when those are needed.

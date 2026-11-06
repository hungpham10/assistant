//! Native codegraph tool surface (`codegraph.call`).
//!
//! Strict session lifecycle, mirroring `codegraph_api::Session`:
//! every conversation starts with `begin(path)`, runs `search` verbs, and
//! finishes with `end()`. Indexing is host-only (CLI `codex codegraph index`
//! and the TUI `/index` command) — the agent never indexes.

#[cfg(feature = "codegraph")]
pub mod service;

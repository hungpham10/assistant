# Codex CLI

[**Codex CLI Documentation**](https://developers.openai.com/codex/cli)

## Workspace layout

Crates are grouped by domain: `codex/` (core engine), `servers/`, `execution/`,
`protocol/` (shared wire types between TUI/CLI/agents and the engine),
`ext/` (extension plugins), `providers/`, `state/`, `observability/`,
`integrations/`, `platform/`, `support/`, `utils/`, `tools/`, `agents/`,
`memories/`, `fs/`, `tui/`, `cli/`.

### Naming convention

Pairs of low-level/engine crates follow one pattern:

- `codex-<name>` — the low-level or shared layer (e.g. `codex-skills` in
  `skills/`-style discovery, `codex-plugin` manifest/registry layer,
  `codex-connectors` data layer).
- `codex-core-<name>` — the engine-side consumer that lives under `codex/`
  (e.g. `codex-core-skills`, `codex-core-plugins`).
- `codex-<name>-extension` — the extension-plugin adapter under `ext/`
  (e.g. `codex-mcp-extension`, `codex-skills-extension`,
  `codex-connectors-extension`).

When adding a new concept, keep a pair consistent with this scheme and prefer
merging single-consumer helper crates into their only dependent instead of
growing the member count.

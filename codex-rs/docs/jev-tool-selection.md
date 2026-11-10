# Jev-Based MCP Tool Selection Design

## Overview

This document describes the design for using Jev (TypeSafe SystemOne) to select MCP tools based on the user's request, rather than advertising all tools upfront.

## Problem

Currently, all MCP tools are advertised to the model every turn. This:
- Bloats the context with irrelevant tools
- Increases token usage
- Makes tool discovery harder for the model

The `tool_search` tool provides a fallback, but the initial tool set should be relevant.

## Solution

Use Jev to rerank the MCP tool pool against the current user question. Top-ranked tools are advertised directly; the rest stay deferred and discoverable via `tool_search`.

## Architecture

### Components

1. **`jev_suggest.rs`** - Core logic for tool suggestion
   - `last_user_question()` - Extracts last user message from history
   - `suggest_direct_mcp_tools()` - Reranks tools via Jev
   - `tool_label()` - Formats tool as `tool \`ns:name\` — description`

2. **`ToolSuggestJevConfig`** - Configuration (opt-in, default disabled)
   - `enabled` - Opt-in flag
   - `model` - Jev model (default: `jev-1.13-free`)
   - `api_key_env` - Env var for API token
   - `direct_tools` - How many top tools to advertise (default: 6)
   - `min_probability` - Threshold for "match" (default: 0.25)
   - `max_candidates` - Max tools sent to Jev (default: 40)
   - `timeout_ms` - Request timeout (default: 2000)

3. **Integration in `turn.rs`** - Called per-turn when `search_tool_enabled`

### Jev Prompt Design

**Current (symbol search style):**
```
State: "Repo: X. Request: Y"
Question: "Which candidate is the code the request is asking about?"
Criteria: { tool1: "tool `ns:tool1` — desc", tool2: "...", none: "..." }
```

**Proposed (planning style for tool selection):**
```
State: "Repo: X. Request: Y

Your task: Select tools needed to accomplish the user's request.
- Choose AT LEAST 2 tools, up to 6
- Consider full workflow (search → edit → test, etc.)
- If no tools relevant, choose 'none_of_the_above'"

Question: "Which tools are needed for this request?"
Criteria: { tool1: "tool `ns:tool1` — desc", ..., none: "..." }
```

### Key Differences

| Aspect | Symbol Search | Tool Selection |
|--------|---------------|----------------|
| Goal | Find matching symbol | Plan tool chain |
| Reasoning | Similarity matching | Task decomposition |
| Min tools | 1 (best match) | Configurable (2+) |
| Prompt style | Retrieval | Planning |

## Configuration

```toml
[tool_suggest_jev]
enabled = true
model = "jev-1.13-free"
api_key_env = "OPENCODE_API_KEY"
direct_tools = 6
min_probability = 0.25
max_candidates = 40
timeout_ms = 2000
```

## Fail-Open Behavior

Every failure path preserves original exposure:
- Disabled config → original
- Missing API token → original
- Timeout/error → original
- No match → all tools deferred
- Empty pool → original

## Future Improvements

1. **Planning prompt** - Use task-decomposition prompt instead of similarity prompt
2. **Min tools config** - Add `min_direct_tools` for mandatory tool count
3. **Context awareness** - Include recent tool usage in state
4. **Tool chains** - Pre-define common tool sequences (search+edit, etc.)
5. **Feedback loop** - Track which suggested tools were actually used

## Related Files

- `codex-rs/codex/core/src/tools/jev_suggest.rs`
- `codex-rs/codex/core/src/tools/jev_suggest_tests.rs`
- `codex-rs/config/src/types.rs` (ToolSuggestJevConfig)
- `codex-rs/codex/core/src/config/mod.rs` (config merge)
- `codex-rs/codex/core/src/session/turn.rs` (integration)
- `codex-rs/codegraph/codegraph-context/src/jev.rs` (shared Jev client)
- `codex-rs/codegraph/codegraph-core/src/jev.rs` (JevConfig)

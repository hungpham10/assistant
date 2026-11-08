//! Question-driven MCP tool suggestion through the Jev decision model.
//!
//! Instead of advertising every MCP tool with the initial request, the current
//! user question is reranked against the MCP tool pool by Jev (TypeSafe
//! SystemOne) — the same optional rerank the codegraph engine uses for symbol
//! search. The top-ranked tools are advertised directly; the rest stay
//! deferred and remain discoverable through `tool_search`.
//!
//! Fail-open everywhere: Jev turned off, missing token, timeout, or any other
//! error keeps the previous [`McpToolExposure`] untouched. Candidate labels
//! are always built in English (tool names and MCP descriptions are English),
//! which is why the question is assumed to be in English too.

use std::collections::HashSet;

use codegraph_context::Candidate;
use codegraph_context::rank as jev_rank;
use codegraph_core::JevConfig;
use codex_config::types::ToolSuggestJevConfig;
use codex_mcp::ToolInfo as McpToolInfo;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;

use crate::mcp_tool_exposure::McpToolExposure;

/// Limit on the question text forwarded to Jev — it rides in `state`.
pub(crate) const MAX_QUESTION_CHARS: usize = 4000;
/// Default number of Jev-ranked tools advertised directly.
pub(crate) const DEFAULT_DIRECT_TOOLS: usize = 6;

/// How many Jev-ranked tools to advertise, from the optional config value.
fn direct_tool_limit(cfg: &ToolSuggestJevConfig) -> usize {
    cfg.direct_tools
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_DIRECT_TOOLS)
}

/// Maps the optional TOML config onto the codegraph Jev client config.
fn to_jev_config(cfg: &ToolSuggestJevConfig) -> JevConfig {
    JevConfig::from_raw(
        Some(cfg.enabled),
        cfg.endpoint.as_deref(),
        cfg.model.as_deref(),
        cfg.api_key_env.as_deref(),
        cfg.timeout_ms,
        cfg.max_candidates,
        cfg.min_probability,
        None,
        None,
        None,
    )
}

/// Extracts the text of the last user message from a sampling input — the
/// question Jev ranks MCP tools against. The returned text is bounded so a
/// pasted document can never blow up the rerank state.
pub(crate) fn last_user_question(input: &[ResponseItem]) -> Option<String> {
    let text = input
        .iter()
        .rev()
        .find_map(|item| match item {
            ResponseItem::Message { role, content, .. } if role == "user" => {
                let text = content
                    .iter()
                    .filter_map(|item| match item {
                        ContentItem::InputText { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(text)
            }
            _ => None,
        })
        .filter(|text| !text.trim().is_empty())?;
    Some(truncate_chars(&text, MAX_QUESTION_CHARS))
}

/// Reranks the MCP pool against `question` and returns a new exposure where
/// the top-ranked tools are advertised directly. `None` means "keep the
/// original exposure": Jev disabled, missing token, ranking failure, or
/// nothing to decide.
pub(crate) async fn suggest_direct_mcp_tools(
    question: &str,
    repo: Option<String>,
    exposure: McpToolExposure,
    cfg: &ToolSuggestJevConfig,
) -> Option<McpToolExposure> {
    if !cfg.enabled {
        return None;
    }
    let pool: Vec<McpToolInfo> = exposure
        .direct_tools
        .iter()
        .cloned()
        .chain(exposure.deferred_tools.iter().flatten().cloned())
        .collect();
    if pool.is_empty() {
        return None;
    }

    let jev_cfg = to_jev_config(cfg);
    if !jev_cfg.is_ready() {
        tracing::debug!(
            api_key_env = %cfg.api_key_env.clone().unwrap_or_else(|| "OPENCODE_API_KEY".into()),
            "tool_suggest_jev skipped — missing token (fail-open)"
        );
        return None;
    }

    let candidates: Vec<Candidate> = pool
        .iter()
        .enumerate()
        .map(|(id, tool)| Candidate {
            id: id as u64,
            label: tool_label(tool),
            detail: None,
        })
        .collect();
    let outcome = jev_rank(question, repo.as_deref(), &candidates, &jev_cfg, None).await;
    if !outcome.used {
        tracing::info!(reason = ?outcome.reason, "tool_suggest_jev skipped — fail-open");
        return None;
    }
    if outcome.no_match {
        tracing::info!("tool_suggest_jev found no relevant MCP tools — everything stays deferred");
        return Some(McpToolExposure {
            direct_tools: Vec::new(),
            deferred_tools: Some(pool),
        });
    }

    let picks: HashSet<usize> = outcome
        .ranking
        .into_iter()
        .take(direct_tool_limit(cfg))
        .map(|(id, _)| id as usize)
        .collect();
    let mut direct = Vec::new();
    let mut deferred = Vec::new();
    for (index, tool) in pool.iter().enumerate() {
        if picks.contains(&index) {
            direct.push(tool.clone());
        } else {
            deferred.push(tool.clone());
        }
    }
    if direct.is_empty() {
        return None;
    }
    tracing::info!(
        direct = %direct
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>()
            .join(","),
        "tool_suggest_jev advertised tools directly for this question"
    );
    Some(McpToolExposure {
        direct_tools: direct,
        deferred_tools: (!deferred.is_empty()).then_some(deferred),
    })
}

/// English label fed to Jev: namespace-qualified name (`namespace:name`) plus
/// description.
fn tool_label(tool: &McpToolInfo) -> String {
    let qualified = match tool.canonical_tool_name().namespace {
        Some(namespace) => format!("{namespace}:{}", tool.callable_name),
        None => tool.callable_name.clone(),
    };
    match tool.tool.description.as_deref() {
        Some(description) if !description.trim().is_empty() => {
            format!(
                "tool `{qualified}` — {}",
                truncate_chars(description.trim(), 200)
            )
        }
        _ => format!("tool `{qualified}`"),
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
#[path = "jev_suggest_tests.rs"]
mod tests;

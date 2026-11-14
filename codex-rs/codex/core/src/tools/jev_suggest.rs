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

use codegraph_core::JevConfig;
use codegraph_core::NONE_OPTION;
use codex_config::types::ToolSuggestJevConfig;
use codex_mcp::ToolInfo as McpToolInfo;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use reqwest::Client;
use serde::Deserialize;
use serde::Serialize;
use std::time::Duration;
use tracing::instrument;

use crate::mcp_tool_exposure::McpToolExposure;
use crate::session::strategy_runtime::StrategyTurnContext;

/// Limit on the question text forwarded to Jev — it rides in `state`.
pub(crate) const MAX_QUESTION_CHARS: usize = 4000;
/// Default number of Jev-ranked tools advertised directly.
pub(crate) const DEFAULT_DIRECT_TOOLS: usize = 6;
/// Default minimum number of tools that must be advertised directly.
pub(crate) const DEFAULT_MIN_DIRECT_TOOLS: usize = 2;

/// How many Jev-ranked tools to advertise, from the optional config value.
fn direct_tool_limit(cfg: &ToolSuggestJevConfig) -> usize {
    cfg.direct_tools
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_DIRECT_TOOLS)
}

/// Minimum number of tools that must be advertised directly.
fn min_direct_tool_limit(cfg: &ToolSuggestJevConfig) -> usize {
    cfg.min_direct_tools
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MIN_DIRECT_TOOLS)
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

/// Outcome of a Jev suggestion pass. `Disabled` means Jev is turned off in
/// config (no event is emitted); `Skipped` carries a human-readable reason so
/// the TUI can explain why Jev did not run this turn (fail-open).
#[derive(Debug, Clone)]
pub(crate) enum JevDecision<S> {
    Disabled,
    Skipped { reason: String },
    Suggested(S),
}

impl<S> JevDecision<S> {
    pub(crate) fn skip_reason(&self) -> Option<&str> {
        match self {
            JevDecision::Disabled | JevDecision::Suggested(_) => None,
            JevDecision::Skipped { reason } => Some(reason),
        }
    }
}

/// Tools Jev advertised directly for one turn, for `/status` tool-usage stats.
/// Names use the TUI tracker format `server/tool`.
#[derive(Debug, Clone)]
pub(crate) struct JevSuggestedTools {
    pub(crate) names: Vec<String>,
    /// Strategy-adjusted probability per advertised tool, sorted descending.
    pub(crate) scores: Vec<(String, f32)>,
    pub(crate) no_match: bool,
}

/// Jev request/response types for tool planning.
#[derive(Serialize)]
struct JevAsk {
    model: String,
    state: String,
    questions: JevQuestions,
}

#[derive(Serialize)]
struct JevQuestions {
    best_tools: JevChoiceQuestion,
    any_fits: JevNoulQuestion,
}

#[derive(Serialize)]
struct JevChoiceQuestion {
    #[serde(rename = "type")]
    kind: &'static str,
    instructions: String,
    criteria: std::collections::HashMap<String, String>,
}

#[derive(Serialize)]
struct JevNoulQuestion {
    #[serde(rename = "type")]
    kind: &'static str,
    instructions: String,
}

#[derive(Deserialize)]
struct JevAnswer {
    #[serde(default)]
    choice: Option<String>,
    #[serde(default)]
    probabilities: std::collections::HashMap<String, f32>,
    #[serde(default)]
    #[allow(dead_code)]
    confidence: Option<f32>,
    #[serde(default)]
    noul: Option<f32>,
}

#[derive(Deserialize)]
struct JevReply {
    #[serde(default)]
    answers: std::collections::HashMap<String, JevAnswer>,
}

/// Outcome of Jev tool planning.
#[derive(Debug, Clone)]
struct JevOutcome {
    used: bool,
    reason: Option<String>,
    ranking: Vec<(u64, f32)>,
    no_match: bool,
    #[allow(dead_code)]
    confidence: Option<f32>,
}

/// Reranks the MCP pool against `question` and returns a new exposure where
/// the top-ranked tools are advertised directly. `None` means "keep the
/// original exposure": Jev disabled, missing token, ranking failure, or
/// nothing to decide.
#[instrument(level = "trace", skip_all)]
pub(crate) async fn suggest_direct_mcp_tools(
    question: &str,
    repo: Option<String>,
    exposure: McpToolExposure,
    cfg: &ToolSuggestJevConfig,
    strategy: Option<&StrategyTurnContext>,
    recent_usage: Option<&str>,
) -> JevDecision<(McpToolExposure, JevSuggestedTools)> {
    if !cfg.enabled {
        return JevDecision::Disabled;
    }
    let pool: Vec<McpToolInfo> = exposure
        .direct_tools
        .iter()
        .cloned()
        .chain(exposure.deferred_tools.iter().flatten().cloned())
        .collect();
    if pool.is_empty() {
        return JevDecision::Skipped {
            reason: "no MCP tools in pool".into(),
        };
    }

    let jev_cfg = to_jev_config(cfg);
    if !jev_cfg.is_ready() {
        tracing::debug!(
            api_key_env = %cfg.api_key_env.clone().unwrap_or_else(|| "OPENCODE_API_KEY".into()),
            "tool_suggest_jev skipped — missing token (fail-open)"
        );
        return JevDecision::Skipped {
            reason: format!(
                "missing {} token",
                cfg.api_key_env
                    .clone()
                    .unwrap_or_else(|| "OPENCODE_API_KEY".into())
            ),
        };
    }

    // Build candidates with labels for Jev; keys mirror the TUI tracker
    // format (`server/tool`) so strategy weights can match them.
    let pool_keys: Vec<String> = pool
        .iter()
        .map(|tool| format!("{}/{}", tool.server_name, tool.tool.name))
        .collect();
    let candidates: Vec<(u64, String)> = pool
        .iter()
        .enumerate()
        .map(|(id, tool)| (id as u64, tool_label(tool)))
        .collect();

    // Call Jev with planning prompt
    let outcome = call_jev_planning(
        question,
        repo.as_deref(),
        &candidates,
        &jev_cfg,
        strategy,
        recent_usage,
    )
    .await;
    if !outcome.used {
        tracing::info!(reason = ?outcome.reason, "tool_suggest_jev skipped — fail-open");
        return JevDecision::Skipped {
            reason: outcome
                .reason
                .unwrap_or_else(|| "Jev ranking unavailable".into()),
        };
    }
    if outcome.no_match {
        tracing::info!("tool_suggest_jev found no relevant MCP tools — everything stays deferred");
        return JevDecision::Suggested((
            McpToolExposure {
                direct_tools: Vec::new(),
                deferred_tools: Some(pool),
            },
            JevSuggestedTools {
                names: Vec::new(),
                scores: Vec::new(),
                no_match: true,
            },
        ));
    }

    // Strategy matrix: multiply Jev's returned percentiles by the active
    // step's tool weights, then renormalize so favored tools compete.
    let ranking = apply_strategy_weights(outcome.ranking, &pool_keys, strategy);

    // Select tools from the strategy-adjusted ranking so weights actually
    // move tools across the `min_probability` threshold.
    let picks = select_direct_ids(
        &ranking,
        jev_cfg.min_probability,
        min_direct_tool_limit(cfg),
        direct_tool_limit(cfg),
    );

    let mut direct = Vec::new();
    let mut deferred = Vec::new();
    let mut suggested = Vec::new();
    for (index, tool) in pool.iter().enumerate() {
        if picks.contains(&index) {
            suggested.push(format!("{}/{}", tool.server_name, tool.tool.name));
            direct.push(tool.clone());
        } else {
            deferred.push(tool.clone());
        }
    }
    if direct.is_empty() {
        return JevDecision::Skipped {
            reason: "Jev selected no MCP tools".into(),
        };
    }
    // Scored view of the advertised tools: strategy-adjusted probability per
    // pick, sorted descending so callers can surface a priority order.
    let rank_by_id: std::collections::HashMap<u64, f32> = ranking.iter().copied().collect();
    let mut scores: Vec<(String, f32)> = direct
        .iter()
        .filter_map(|tool| {
            let key = format!("{}/{}", tool.server_name, tool.tool.name);
            pool_keys
                .iter()
                .position(|candidate| *candidate == key)
                .and_then(|index| rank_by_id.get(&(index as u64)).copied())
                .map(|prob| (key, prob))
        })
        .collect();
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    tracing::info!(
        direct = %direct
            .iter()
            .map(|tool| tool.callable_name.as_str())
            .collect::<Vec<_>>()
            .join(","),
        "tool_suggest_jev advertised tools directly for this question"
    );
    JevDecision::Suggested((
        McpToolExposure {
            direct_tools: direct,
            deferred_tools: (!deferred.is_empty()).then_some(deferred),
        },
        JevSuggestedTools {
            names: suggested,
            scores,
            no_match: false,
        },
    ))
}

/// Calls Jev with a planning prompt for tool selection. The optional
/// strategy HMM context and recent tool usage bias the planning state.
pub(crate) async fn call_jev_planning(
    question: &str,
    repo: Option<&str>,
    candidates: &[(u64, String)],
    cfg: &JevConfig,
    strategy: Option<&StrategyTurnContext>,
    recent_usage: Option<&str>,
) -> JevOutcome {
    let api_key = match cfg.api_key() {
        Some(key) => key,
        None => {
            return JevOutcome {
                used: false,
                reason: Some("missing API key".into()),
                ranking: Vec::new(),
                no_match: false,
                confidence: None,
            };
        }
    };

    // Build criteria map
    let mut criteria = std::collections::HashMap::with_capacity(candidates.len() + 1);
    for (id, label) in candidates {
        criteria.insert(format!("s{id}"), label.clone());
    }
    criteria.insert(
        NONE_OPTION.to_string(),
        "No candidate implements the requested behaviour".into(),
    );

    // Build planning state prompt
    let state = build_planning_state(question, repo, candidates.len(), strategy, recent_usage);

    // Build Jev request
    let ask = JevAsk {
        model: cfg.model.clone(),
        state,
        questions: JevQuestions {
            best_tools: JevChoiceQuestion {
                kind: "choice",
                instructions: "Which tools are needed to accomplish the user's request? Select the tools that form a complete workflow.".into(),
                criteria,
            },
            any_fits: JevNoulQuestion {
                kind: "noul",
                instructions: "Does any combination of candidates accomplish the behaviour described in the request?".into(),
            },
        },
    };

    let body = match serde_json::to_vec(&ask) {
        Ok(b) => b,
        Err(e) => {
            return JevOutcome {
                used: false,
                reason: Some(format!("serialize request Jev: {e}")),
                ranking: Vec::new(),
                no_match: false,
                confidence: None,
            };
        }
    };

    let client = match Client::builder()
        .timeout(Duration::from_millis(cfg.timeout_ms))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return JevOutcome {
                used: false,
                reason: Some(format!("create HTTP client: {e}")),
                ranking: Vec::new(),
                no_match: false,
                confidence: None,
            };
        }
    };

    let resp = match client
        .post(&cfg.endpoint)
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {api_key}"))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "Jev HTTP request failed");
            return JevOutcome {
                used: false,
                reason: Some(format!("Jev HTTP error: {e}")),
                ranking: Vec::new(),
                no_match: false,
                confidence: None,
            };
        }
    };

    let text = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            return JevOutcome {
                used: false,
                reason: Some(format!("Jev response read error: {e}")),
                ranking: Vec::new(),
                no_match: false,
                confidence: None,
            };
        }
    };

    parse_jev_reply(&text, candidates)
}

/// Builds the planning state prompt for Jev. The optional strategy context
/// injects the active `/plan <strategy>` HMM state (current step, guidance,
/// favored tools) and the optional recent tool usage snippet makes the
/// ranking sequence-aware, so Jev's probability distribution leans toward
/// tools that continue the workflow.
pub(crate) fn build_planning_state(
    question: &str,
    repo: Option<&str>,
    num_candidates: usize,
    strategy: Option<&StrategyTurnContext>,
    recent_usage: Option<&str>,
) -> String {
    let repo_part = repo
        .filter(|r| !r.trim().is_empty())
        .map(|r| format!("Repo: {r}. "))
        .unwrap_or_default();
    let strategy_part = strategy.map(strategy_hints).unwrap_or_default();
    let recent_part = recent_usage
        .map(|usage| {
            format!(
                "Recent tool usage (most recent last): {}\n",
                truncate_chars(usage.trim(), MAX_RECENT_TOOL_USAGE_CHARS)
            )
        })
        .unwrap_or_default();

    format!(
        "{repo_part}Request: {question}

{recent_part}{strategy_part}Your task: Select the tools needed to accomplish the user's request.
- Choose AT LEAST 2 tools, up to 6
- Consider the full workflow (search → edit → test, etc.)
- Tools should form a complete chain to fulfill the request
- Prefer tools that continue the workflow suggested by the recent tool usage and the current strategy step
- If no tools are relevant, choose 'none_of_the_above'

Available tools ({num_candidates} candidates) are listed in the criteria below."
    )
}

/// Renders the strategy HMM hints (step node, guidance, favored tools) for
/// the planning state.
fn strategy_hints(strategy: &StrategyTurnContext) -> String {
    let mut text = format!(
        "Active strategy `{}` — current step: {}\n\
         Step guidance (a suggestion, not a mandate): {}\n",
        strategy.name,
        truncate_chars(strategy.node.trim(), MAX_STRATEGY_NODE_CHARS),
        truncate_chars(strategy.guidance.trim(), MAX_STRATEGY_GUIDANCE_CHARS)
    );
    let mut favored: Vec<String> = strategy
        .tool_weights
        .iter()
        .filter(|(_, weight)| **weight > 1.0)
        .map(|(tool, weight)| format!("{tool} ({weight:.1}x)"))
        .collect();
    for tool in &strategy.strong_prior_tools {
        if !favored.iter().any(|entry| entry.starts_with(tool.as_str())) {
            favored.push(tool.clone());
        }
    }
    favored.truncate(MAX_STRATEGY_FAVORED_TOOLS);
    if !favored.is_empty() {
        text.push_str(&format!("Strategy-favored tools: {}\n", favored.join(", ")));
    }
    text.push_str("Prefer the candidates that fit this step of the strategy.\n");
    text
}

/// Cap on the strategy guidance snippet injected into the Jev state.
pub(crate) const MAX_STRATEGY_GUIDANCE_CHARS: usize = 300;
/// Cap on the strategy step node name injected into the Jev state.
const MAX_STRATEGY_NODE_CHARS: usize = 60;
/// Cap on the number of strategy-favored tools listed in the Jev state.
const MAX_STRATEGY_FAVORED_TOOLS: usize = 5;
/// Cap on the recent tool usage snippet injected into the Jev state — the
/// Jev model is small, so the sequence hint stays bounded.
const MAX_RECENT_TOOL_USAGE_CHARS: usize = 200;

/// Multiplies Jev's returned percentiles by the active step's tool weights
/// and renormalizes, so strategy-favored tools compete for direct slots.
/// Unmatched tools keep weight 1.0; the ranking is untouched without a
/// strategy or an empty weight row.
pub(crate) fn apply_strategy_weights(
    ranking: Vec<(u64, f32)>,
    pool_keys: &[String],
    strategy: Option<&StrategyTurnContext>,
) -> Vec<(u64, f32)> {
    let Some(strategy) = strategy else {
        return ranking;
    };
    if strategy.tool_weights.is_empty() || ranking.is_empty() {
        return ranking;
    }
    let weight_for = |pool_key: &str| {
        strategy
            .tool_weights
            .iter()
            .find(|(authored, _)| {
                codex_strategy::tool_key_matches(pool_key, authored)
                    || codex_strategy::tool_key_matches(authored, pool_key)
            })
            .map(|(_, weight)| *weight)
            .unwrap_or(1.0)
    };
    let mut adjusted: Vec<(u64, f32)> = ranking
        .into_iter()
        .map(|(id, prob)| {
            let weight = pool_keys
                .get(id as usize)
                .map(|key| weight_for(key))
                .unwrap_or(1.0)
                .max(0.05);
            (id, prob * weight)
        })
        .collect();
    let total: f32 = adjusted.iter().map(|(_, p)| p).sum();
    if total > 0.0 && total.is_finite() {
        for (_, p) in adjusted.iter_mut() {
            *p /= total;
        }
    }
    adjusted
}

/// Picks the pool indices to advertise directly from the (strategy-adjusted)
/// ranking: first pass takes tools at or above `min_prob` in rank order,
/// second pass fills up to `min_tools` regardless of probability, capped at
/// `max_tools` overall.
fn select_direct_ids(
    ranking: &[(u64, f32)],
    min_prob: f32,
    min_tools: usize,
    max_tools: usize,
) -> HashSet<usize> {
    let mut picks: HashSet<usize> = HashSet::new();

    // First pass: tools above probability threshold
    for (id, prob) in ranking {
        if *prob >= min_prob && picks.len() < max_tools {
            picks.insert(*id as usize);
        }
    }

    // Second pass: fill up to min_direct_tools from ranked list regardless of probability
    for (id, _) in ranking {
        if picks.len() >= min_tools {
            break;
        }
        if picks.len() < max_tools {
            picks.insert(*id as usize);
        }
    }

    picks.into_iter().take(max_tools).collect()
}
/// Parses Jev reply into outcome.
pub(crate) fn parse_jev_reply(raw: &str, candidates: &[(u64, String)]) -> JevOutcome {
    let reply: JevReply = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(e) => {
            return JevOutcome {
                used: false,
                reason: Some(format!("Jev JSON parse error: {e}")),
                ranking: Vec::new(),
                no_match: false,
                confidence: None,
            };
        }
    };

    let best = match reply.answers.get("best_tools") {
        Some(b) => b,
        None => {
            return JevOutcome {
                used: false,
                reason: Some("Jev missing answer 'best_tools'".into()),
                ranking: Vec::new(),
                no_match: false,
                confidence: None,
            };
        }
    };

    let _fits = reply
        .answers
        .get("any_fits")
        .and_then(|a| a.noul)
        .unwrap_or(0.0);

    let none_probability = best.probabilities.get(NONE_OPTION).copied().unwrap_or(0.0);
    let mut ranking: Vec<(u64, f32)> = candidates
        .iter()
        .filter_map(|(id, _)| {
            let p = best
                .probabilities
                .get(&format!("s{id}"))
                .copied()
                .unwrap_or(0.0);
            (p > 0.0).then_some((*id, p))
        })
        .collect();
    ranking.sort_by(|a, b| b.1.total_cmp(&a.1));

    let top = ranking.first().map(|(_, p)| *p).unwrap_or(0.0);
    let no_match = best.choice.as_deref() == Some(NONE_OPTION)
        || ranking.is_empty()
        || none_probability >= top;

    JevOutcome {
        used: true,
        reason: None,
        ranking,
        no_match,
        confidence: best.confidence,
    }
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

// ============================================================
// Universal Jev Tool Selection (ALL tool types)
// ============================================================

/// Tool type for Jev selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolType {
    Mcp,
    Builtin,
    Extension,
    Dynamic,
    CodeMode,
}

/// Tool entry for Jev selection with full metadata.
#[derive(Debug, Clone)]
pub(crate) struct JevToolEntry {
    pub id: u64,
    pub name: String,
    pub namespace: Option<String>,
    pub description: String,
    pub tool_type: ToolType,
    pub essential: bool,
}

/// Result of universal tool suggestion.
#[derive(Debug, Clone)]
pub(crate) struct UniversalToolSuggestion {
    pub direct_tools: Vec<JevToolEntry>,
    pub deferred_tools: Vec<JevToolEntry>,
    pub suggested_names: Vec<String>,
    /// Strategy-adjusted probability per advertised tool, sorted descending.
    /// Keys use the `namespace/name` form (namespace defaults to `builtin`).
    pub scores: Vec<(String, f32)>,
    pub no_match: bool,
}

/// Result of the universal Jev routing pass performed while planning tools.
pub(crate) struct JevRoutingOutcome {
    pub(crate) suggested: Option<UniversalToolSuggestion>,
    pub(crate) skip_reason: Option<String>,
}

impl JevRoutingOutcome {
    pub(crate) fn none() -> Self {
        Self {
            suggested: None,
            skip_reason: None,
        }
    }
}

/// Universal tool suggestion entry point.
/// Reranks the FULL tool pool against `question` and returns selected tools.
/// Fail-open: any skip keeps the caller's original exposure untouched and the
/// reason is surfaced so the TUI can explain why Jev did not run.
#[instrument(level = "trace", skip_all)]
pub(crate) async fn suggest_direct_tools(
    question: &str,
    repo: Option<String>,
    all_tools: Vec<JevToolEntry>,
    cfg: &ToolSuggestJevConfig,
    strategy: Option<&StrategyTurnContext>,
    recent_usage: Option<&str>,
) -> JevDecision<UniversalToolSuggestion> {
    if !cfg.enabled {
        return JevDecision::Disabled;
    }
    if all_tools.is_empty() {
        return JevDecision::Skipped {
            reason: "no tools in pool".into(),
        };
    }

    let jev_cfg = to_jev_config(cfg);
    if !jev_cfg.is_ready() {
        tracing::debug!(
            api_key_env = %cfg.api_key_env.clone().unwrap_or_else(|| "OPENCODE_API_KEY".into()),
            "tool_suggest_jev skipped — missing token (fail-open)"
        );
        return JevDecision::Skipped {
            reason: format!(
                "missing {} token",
                cfg.api_key_env
                    .clone()
                    .unwrap_or_else(|| "OPENCODE_API_KEY".into())
            ),
        };
    }

    // Separate essential tools (always direct) from selectable tools
    let (essential_tools, selectable_tools): (Vec<_>, Vec<_>) =
        all_tools.into_iter().partition(|t| t.essential);

    // Compute suggested_names for essential tools before any moves
    let essential_names: Vec<String> = essential_tools
        .iter()
        .map(|t| format!("{}/{}", t.namespace.as_deref().unwrap_or("builtin"), t.name))
        .collect();

    // Build candidates for Jev (only selectable tools)
    let candidates: Vec<(u64, String)> = selectable_tools
        .iter()
        .enumerate()
        .map(|(idx, tool)| (idx as u64, tool_label_universal(tool)))
        .collect();

    if candidates.is_empty() {
        // Only essential tools - return them as direct
        return JevDecision::Suggested(UniversalToolSuggestion {
            direct_tools: essential_tools,
            deferred_tools: Vec::new(),
            suggested_names: essential_names,
            scores: Vec::new(),
            no_match: false,
        });
    }

    // Call Jev with planning prompt
    let outcome = call_jev_planning(
        question,
        repo.as_deref(),
        &candidates,
        &jev_cfg,
        strategy,
        recent_usage,
    )
    .await;
    if !outcome.used {
        tracing::info!(reason = ?outcome.reason, "tool_suggest_jev skipped — fail-open");
        return JevDecision::Skipped {
            reason: outcome
                .reason
                .unwrap_or_else(|| "Jev ranking unavailable".into()),
        };
    }
    if outcome.no_match {
        tracing::info!(
            "tool_suggest_jev found no relevant tools — essentials stay direct, rest deferred"
        );
        return JevDecision::Suggested(UniversalToolSuggestion {
            direct_tools: essential_tools,
            deferred_tools: selectable_tools,
            suggested_names: essential_names,
            scores: Vec::new(),
            no_match: true,
        });
    }

    // Strategy matrix: multiply Jev's returned percentiles by the active
    // step's tool weights, then renormalize so favored tools compete.
    let pool_keys: Vec<String> = candidates
        .iter()
        .zip(selectable_tools.iter())
        .map(|(_, tool)| {
            format!(
                "{}/{}",
                tool.namespace.as_deref().unwrap_or("builtin"),
                tool.name
            )
        })
        .collect();
    let ranking = apply_strategy_weights(outcome.ranking, &pool_keys, strategy);

    // Select tools: first take those above min_probability, then fill up to min_direct_tools
    let min_prob = jev_cfg.min_probability;
    let min_tools = min_direct_tool_limit(cfg);
    let max_tools = direct_tool_limit(cfg);
    let picks = select_direct_ids(&ranking, min_prob, min_tools, max_tools);

    // Build direct and deferred lists
    let mut direct = essential_tools;
    let mut deferred = Vec::new();
    let mut suggested = essential_names;
    let rank_by_id: std::collections::HashMap<u64, f32> = ranking.iter().copied().collect();

    let mut scores: Vec<(String, f32)> = Vec::new();
    for (idx, tool) in selectable_tools.into_iter().enumerate() {
        let key = pool_keys[idx].clone();
        if picks.contains(&idx) {
            suggested.push(key.clone());
            scores.push((key, rank_by_id.get(&(idx as u64)).copied().unwrap_or(0.0)));
            direct.push(tool);
        } else {
            deferred.push(tool);
        }
    }
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));

    if direct.is_empty() {
        return JevDecision::Skipped {
            reason: "Jev selected no tools".into(),
        };
    }
    tracing::info!(
        direct = %direct
            .iter()
            .map(|tool| format!("{}/{}", tool.namespace.as_deref().unwrap_or("builtin"), tool.name))
            .collect::<Vec<_>>()
            .join(","),
        "tool_suggest_jev advertised tools directly for this question"
    );
    JevDecision::Suggested(UniversalToolSuggestion {
        direct_tools: direct,
        deferred_tools: deferred,
        suggested_names: suggested,
        scores,
        no_match: false,
    })
}

/// English label fed to Jev: includes tool type and namespace-qualified name plus description.
fn tool_label_universal(tool: &JevToolEntry) -> String {
    let type_prefix = match tool.tool_type {
        ToolType::Mcp => "mcp",
        ToolType::Builtin => "builtin",
        ToolType::Extension => "ext",
        ToolType::Dynamic => "dyn",
        ToolType::CodeMode => "codemode",
    };
    let qualified = match &tool.namespace {
        Some(ns) => format!("{}:{}", ns, tool.name),
        None => tool.name.clone(),
    };
    let desc = tool.description.as_str();
    if !desc.trim().is_empty() {
        format!(
            "{type_prefix} `{qualified}` — {}",
            truncate_chars(desc.trim(), 150)
        )
    } else {
        format!("{type_prefix} `{qualified}`")
    }
}

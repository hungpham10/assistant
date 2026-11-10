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
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::instrument;

use crate::mcp_tool_exposure::McpToolExposure;

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

/// Tools Jev advertised directly for one turn, for `/status` tool-usage stats.
/// Names use the TUI tracker format `server/tool`.
#[derive(Debug, Clone)]
pub(crate) struct JevSuggestedTools {
    pub(crate) names: Vec<String>,
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
) -> Option<(McpToolExposure, JevSuggestedTools)> {
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

    // Build candidates with labels for Jev
    let candidates: Vec<(u64, String)> = pool
        .iter()
        .enumerate()
        .map(|(id, tool)| (id as u64, tool_label(tool)))
        .collect();

    // Call Jev with planning prompt
    let outcome = call_jev_planning(question, repo.as_deref(), &candidates, &jev_cfg).await;
    if !outcome.used {
        tracing::info!(reason = ?outcome.reason, "tool_suggest_jev skipped — fail-open");
        return None;
    }
    if outcome.no_match {
        tracing::info!("tool_suggest_jev found no relevant MCP tools — everything stays deferred");
        return Some((
            McpToolExposure {
                direct_tools: Vec::new(),
                deferred_tools: Some(pool),
            },
            JevSuggestedTools {
                names: Vec::new(),
                no_match: true,
            },
        ));
    }

    // Select tools: first take those above min_probability, then fill up to min_direct_tools
    let min_prob = jev_cfg.min_probability;
    let min_tools = min_direct_tool_limit(cfg);
    let max_tools = direct_tool_limit(cfg);

    let mut picks: HashSet<usize> = HashSet::new();

    // First pass: tools above probability threshold
    for (id, prob) in &outcome.ranking {
        if *prob >= min_prob && picks.len() < max_tools {
            picks.insert(*id as usize);
        }
    }

    // Second pass: fill up to min_direct_tools from ranked list regardless of probability
    for (id, _) in &outcome.ranking {
        if picks.len() >= min_tools {
            break;
        }
        if picks.len() < max_tools {
            picks.insert(*id as usize);
        }
    }

    // Cap at max_tools
    let picks: Vec<usize> = picks.into_iter().take(max_tools).collect();

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
    Some((
        McpToolExposure {
            direct_tools: direct,
            deferred_tools: (!deferred.is_empty()).then_some(deferred),
        },
        JevSuggestedTools {
            names: suggested,
            no_match: false,
        },
    ))
}

/// Calls Jev with a planning prompt for tool selection.
pub(crate) async fn call_jev_planning(
    question: &str,
    repo: Option<&str>,
    candidates: &[(u64, String)],
    cfg: &JevConfig,
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
        criteria.insert(format!("s{}", id), label.clone());
    }
    criteria.insert(
        NONE_OPTION.to_string(),
        "No candidate implements the requested behaviour".into(),
    );

    // Build planning state prompt
    let state = build_planning_state(question, repo, candidates.len());

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

/// Builds the planning state prompt for Jev.
pub(crate) fn build_planning_state(
    question: &str,
    repo: Option<&str>,
    num_candidates: usize,
) -> String {
    let repo_part = repo
        .filter(|r| !r.trim().is_empty())
        .map(|r| format!("Repo: {r}. "))
        .unwrap_or_default();

    format!(
        "{repo_part}Request: {question}

Your task: Select the tools needed to accomplish the user's request.
- Choose AT LEAST 2 tools, up to 6
- Consider the full workflow (search → edit → test, etc.)
- Tools should form a complete chain to fulfill the request
- If no tools are relevant, choose 'none_of_the_above'

Available tools ({num_candidates} candidates) are listed in the criteria below."
    )
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
                .get(&format!("s{}", id))
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

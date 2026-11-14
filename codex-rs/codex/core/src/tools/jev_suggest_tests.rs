//! Tests cho `jev_suggest` — phần thuần (mapping, fail-open) chạy offline.

use super::JevDecision;
use super::build_planning_state;
use super::last_user_question;
use super::parse_jev_reply;
use super::suggest_direct_mcp_tools;
use super::tool_label;
use super::truncate_chars;
use crate::mcp_tool_exposure::McpToolExposure;
use codex_config::types::ToolSuggestJevConfig;
use codex_mcp::ToolInfo as McpToolInfo;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;
use std::sync::Arc;

fn plain_tool(name: &str, description: &str) -> McpToolInfo {
    McpToolInfo {
        server_name: "srv".into(),
        supports_parallel_tool_calls: false,
        server_origin: None,
        callable_name: name.into(),
        callable_namespace: "srv".into(),
        namespace_description: None,
        tool: rmcp::model::Tool::new(name.to_string(), description.to_string(), Arc::default()),
        connector_id: None,
        connector_name: None,
        plugin_display_names: Vec::new(),
    }
}

fn user_message(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "user".into(),
        content: vec![ContentItem::InputText { text: text.into() }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn last_user_question_lấy_message_user_cuối() {
    let items = vec![
        user_message("first"),
        user_message("second — a longer question about git"),
    ];
    assert_eq!(
        last_user_question(&items).as_deref(),
        Some("second — a longer question about git")
    );
}

#[test]
fn last_user_question_rồng_istringstream_trả_none() {
    assert_eq!(last_user_question(&[]), None);
    assert_eq!(last_user_question(&[user_message("   ")]), None);
}

#[test]
fn truncate_chars_chíhw_số_lý_hình() {
    assert_eq!(truncate_chars("abc", 10), "abc");
    assert_eq!(truncate_chars("abcdef", 3), "abc…");
}

#[test]
fn tool_label_ghép_final_descripton_tiếng_anh() {
    let tool = plain_tool("run", "Run a shell command");
    assert_eq!(tool_label(&tool), "tool `srv:run` — Run a shell command");
}

#[tokio::test]
async fn tắt_hoặc_thiếu_token_thì_fail_open_trả_none() {
    let exposure = McpToolExposure {
        direct_tools: Vec::new(),
        deferred_tools: Some(vec![plain_tool("run", "Run a shell command")]),
    };
    // Config tắt mặc định → None, exposure giữ nguyên.
    let out = suggest_direct_mcp_tools(
        "run a shell command",
        None,
        exposure.clone(),
        &ToolSuggestJevConfig::default(),
        /*strategy*/ None,
    )
    .await;
    assert!(matches!(out, JevDecision::Skipped { .. }));

    // Enabled nhưng thiếu token env → vẫn fail-open.
    let cfg = ToolSuggestJevConfig {
        enabled: true,
        api_key_env: Some("CODEX_TEST_JEV_SUGGEST_ABSENT_KEY".into()),
        ..ToolSuggestJevConfig::default()
    };
    let out = suggest_direct_mcp_tools(
        "run a shell command",
        None,
        exposure,
        &cfg,
        /*strategy*/ None,
    )
    .await;
    assert!(matches!(out, JevDecision::Skipped { .. }));
}

#[tokio::test]
async fn pool_rỗng_thì_trả_none() {
    let cfg = ToolSuggestJevConfig {
        enabled: true,
        api_key_env: Some("CODEX_TEST_JEV_SUGGEST_ABSENT_KEY".into()),
        ..ToolSuggestJevConfig::default()
    };
    let exposure = McpToolExposure {
        direct_tools: Vec::new(),
        deferred_tools: None,
    };
    // Pool rỗng → không có gì để rerank, coi như None dù enabled.
    let out = suggest_direct_mcp_tools("anything", None, exposure, &cfg, /*strategy*/ None).await;
    assert!(matches!(out, JevDecision::Skipped { .. }));
}

#[test]
fn build_planning_state_includes_guidance() {
    let state = build_planning_state("run tests", Some("my-repo"), 5, /*strategy*/ None);
    assert!(state.contains("AT LEAST 2 tools"));
    assert!(state.contains("up to 6"));
    assert!(state.contains("search → edit → test"));
    assert!(state.contains("complete chain"));
    assert!(state.contains("none_of_the_above"));
    assert!(state.contains("my-repo"));
    assert!(state.contains("run tests"));
    assert!(state.contains("5 candidates"));
}

#[test]
fn build_planning_state_without_repo() {
    let state = build_planning_state("run tests", None, 3, /*strategy*/ None);
    assert!(!state.contains("Repo:"));
    assert!(state.contains("Request: run tests"));
    assert!(state.contains("3 candidates"));
}

#[test]
fn parse_jev_reply_ranks_by_probability() {
    let candidates = vec![
        (0, "tool `srv:run` — Run command".into()),
        (1, "tool `srv:search` — Search files".into()),
        (2, "tool `srv:edit` — Edit file".into()),
    ];

    let reply = r#"{
        "answers": {
            "best_tools": {
                "choice": "s1",
                "probabilities": {
                    "s0": 0.15,
                    "s1": 0.65,
                    "s2": 0.10,
                    "none_of_the_above": 0.10
                },
                "confidence": 0.82
            },
            "any_fits": {"noul": 0.3}
        }
    }"#;

    let outcome = parse_jev_reply(reply, &candidates);
    assert!(outcome.used);
    assert!(!outcome.no_match);
    assert_eq!(outcome.ranking.len(), 3);
    assert_eq!(outcome.ranking[0], (1, 0.65));
    assert_eq!(outcome.ranking[1], (0, 0.15));
    assert_eq!(outcome.ranking[2], (2, 0.10));
    assert!((outcome.confidence.unwrap() - 0.82).abs() < 0.01);
}

#[test]
fn parse_jev_reply_none_wins_returns_no_match() {
    let candidates = vec![(0, "tool `srv:run` — Run command".into())];

    let reply = r#"{
        "answers": {
            "best_tools": {
                "choice": "none_of_the_above",
                "probabilities": {
                    "s0": 0.20,
                    "none_of_the_above": 0.80
                },
                "confidence": 0.9
            },
            "any_fits": {"noul": 0.9}
        }
    }"#;

    let outcome = parse_jev_reply(reply, &candidates);
    assert!(outcome.used);
    assert!(outcome.no_match);
    assert!(outcome.no_match);
}

#[test]
fn parse_jev_reply_none_probability_ge_top_returns_no_match() {
    let candidates = vec![
        (0, "tool `srv:run` — Run command".into()),
        (1, "tool `srv:search` — Search files".into()),
    ];

    let reply = r#"{
        "answers": {
            "best_tools": {
                "choice": "s0",
                "probabilities": {
                    "s0": 0.30,
                    "s1": 0.20,
                    "none_of_the_above": 0.50
                },
                "confidence": 0.7
            },
            "any_fits": {"noul": 0.5}
        }
    }"#;

    let outcome = parse_jev_reply(reply, &candidates);
    assert!(outcome.used);
    assert!(outcome.no_match);
    assert!(outcome.no_match);
}

#[test]
fn parse_jev_reply_missing_best_tools_returns_fail_open() {
    let candidates = vec![(0, "tool `srv:run`".into())];

    let reply = r#"{"answers": {"any_fits": {"noul": 0.5}}}"#;

    let outcome = parse_jev_reply(reply, &candidates);
    assert!(!outcome.used);
    assert!(outcome.reason.unwrap().contains("best_tools"));
}

#[test]
fn parse_jev_reply_invalid_json_returns_fail_open() {
    let candidates = vec![(0, "tool `srv:run`".into())];

    let reply = "not valid json";

    let outcome = parse_jev_reply(reply, &candidates);
    assert!(!outcome.used);
    assert!(outcome.reason.unwrap().contains("JSON parse"));
}

fn create_test_jev_config(
    enabled: bool,
    min_prob: Option<f32>,
    min_direct: Option<usize>,
    max_direct: Option<usize>,
) -> ToolSuggestJevConfig {
    ToolSuggestJevConfig {
        enabled,
        endpoint: Some("https://test.example.com".into()),
        model: Some("jev-1.13-free".into()),
        api_key_env: Some("TEST_KEY".into()),
        timeout_ms: Some(5000),
        max_candidates: Some(40),
        min_probability: min_prob,
        direct_tools: max_direct,
        min_direct_tools: min_direct,
        include_all_tools: true,
        essential_tools: Vec::new(),
    }
}

fn create_test_pool() -> McpToolExposure {
    McpToolExposure {
        direct_tools: Vec::new(),
        deferred_tools: Some(vec![
            plain_tool("run", "Run a shell command"),
            plain_tool("search", "Search files in workspace"),
            plain_tool("edit", "Edit a file"),
            plain_tool("test", "Run tests"),
        ]),
    }
}

#[tokio::test]
async fn suggest_direct_mcp_tools_with_min_direct_tools() {
    let cfg = create_test_jev_config(true, Some(0.25), Some(2), Some(6));
    let exposure = create_test_pool();

    // With no API key, should fail-open (return None)
    let result = suggest_direct_mcp_tools(
        "run the tests",
        Some("test-repo".into()),
        exposure,
        &cfg,
        /*strategy*/ None,
    )
    .await;

    // Should fail-open because test key doesn't exist
    assert!(matches!(result, JevDecision::Skipped { .. }));
}

#[test]
fn direct_tool_limit_uses_config_or_default() {
    let cfg = ToolSuggestJevConfig {
        direct_tools: Some(4),
        ..ToolSuggestJevConfig::default()
    };
    assert_eq!(super::direct_tool_limit(&cfg), 4);

    let cfg = ToolSuggestJevConfig::default();
    assert_eq!(super::direct_tool_limit(&cfg), 6); // DEFAULT_DIRECT_TOOLS

    let cfg = ToolSuggestJevConfig {
        direct_tools: Some(0),
        ..ToolSuggestJevConfig::default()
    };
    assert_eq!(super::direct_tool_limit(&cfg), 6); // filters out <= 0
}

#[test]
fn min_direct_tool_limit_uses_config_or_default() {
    let cfg = ToolSuggestJevConfig {
        min_direct_tools: Some(3),
        ..ToolSuggestJevConfig::default()
    };
    assert_eq!(super::min_direct_tool_limit(&cfg), 3);

    let cfg = ToolSuggestJevConfig::default();
    assert_eq!(super::min_direct_tool_limit(&cfg), 2); // DEFAULT_MIN_DIRECT_TOOLS

    let cfg = ToolSuggestJevConfig {
        min_direct_tools: Some(0),
        ..ToolSuggestJevConfig::default()
    };
    assert_eq!(super::min_direct_tool_limit(&cfg), 2); // filters out <= 0
}

#[test]
fn to_jev_config_maps_all_fields() {
    let cfg = ToolSuggestJevConfig {
        enabled: true,
        endpoint: Some("https://custom.example.com".into()),
        model: Some("jev-1.13".into()),
        api_key_env: Some("CUSTOM_KEY".into()),
        timeout_ms: Some(3000),
        max_candidates: Some(50),
        min_probability: Some(0.3),
        direct_tools: Some(4),
        min_direct_tools: Some(2),
        include_all_tools: true,
        essential_tools: Vec::new(),
    };

    let jev_cfg = super::to_jev_config(&cfg);
    assert!(jev_cfg.enabled);
    assert_eq!(jev_cfg.endpoint, "https://custom.example.com");
    assert_eq!(jev_cfg.model, "jev-1.13");
    assert_eq!(jev_cfg.api_key_env, "CUSTOM_KEY");
    assert_eq!(jev_cfg.timeout_ms, 3000);
    assert_eq!(jev_cfg.max_candidates, 50);
    assert!((jev_cfg.min_probability - 0.3).abs() < 0.001);
}

#[test]
fn build_planning_state_includes_strategy_preamble() {
    let state = build_planning_state(
        "trace the flow",
        Some("my-repo"),
        4,
        Some(("explore-code", "Trace flows from entrypoints.")),
    );
    assert!(state.contains("Active strategy `explore-code`"));
    assert!(state.contains("Trace flows from entrypoints."));
    assert!(state.contains("Prefer the candidates that fit this step"));
    assert!(state.contains("trace the flow"));
}

#[test]
fn build_planning_state_truncates_strategy_guidance() {
    let guidance = "x".repeat(500);
    let state = build_planning_state("q", None, 2, Some(("s", &guidance)));
    let injected = state
        .lines()
        .find(|line| line.starts_with("Active strategy"))
        .expect("strategy line");
    assert!(injected.len() < 500, "guidance snippet must be capped");
}

#[test]
fn strategy_weights_boost_favored_tools() {
    use super::apply_strategy_weights;
    use crate::session::strategy_runtime::StrategyTurnContext;
    use std::collections::HashMap;

    let pool_keys = vec!["srv/flow".to_string(), "srv/edit".to_string()];
    let ranking = vec![(0u64, 0.5f32), (1u64, 0.5f32)];
    let mut weights = HashMap::new();
    weights.insert("srv/flow".to_string(), 3.0f32);
    weights.insert("srv/edit".to_string(), 1.0f32);
    let strategy = StrategyTurnContext {
        name: "explore-code".into(),
        node: "entrypoint-flow".into(),
        guidance: String::new(),
        tool_weights: weights,
        strong_prior_tools: vec!["srv/flow".into()],
    };

    let adjusted = apply_strategy_weights(ranking, &pool_keys, Some(&strategy));
    let flow = adjusted.iter().find(|(id, _)| *id == 0).map(|(_, p)| *p);
    let edit = adjusted.iter().find(|(id, _)| *id == 1).map(|(_, p)| *p);
    assert!(
        flow.unwrap() > edit.unwrap(),
        "favored tool must overtake: {adjusted:?}"
    );
    let total: f32 = adjusted.iter().map(|(_, p)| p).sum();
    assert!((total - 1.0).abs() < 1e-4, "weights renormalize: {total}");
}

#[test]
fn strategy_weights_without_match_keep_ranking() {
    use super::apply_strategy_weights;
    use crate::session::strategy_runtime::StrategyTurnContext;

    let pool_keys = vec!["srv/other".to_string()];
    let ranking = vec![(0u64, 0.7f32)];
    let mut weights = std::collections::HashMap::new();
    weights.insert("srv/flow".to_string(), 3.0f32);
    let strategy = StrategyTurnContext {
        name: "s".into(),
        node: "n".into(),
        guidance: String::new(),
        tool_weights: weights,
        strong_prior_tools: Vec::new(),
    };
    let adjusted = apply_strategy_weights(ranking.clone(), &pool_keys, Some(&strategy));
    assert!(
        (adjusted[0].1 - 1.0).abs() < 1e-4,
        "unmatched tools renormalize to 1: {adjusted:?}"
    );
    assert_eq!(
        apply_strategy_weights(ranking, &pool_keys, None),
        vec![(0u64, 0.7f32)],
        "no strategy leaves the ranking untouched"
    );
}

#[test]
fn select_direct_ids_threshold_then_fill() {
    use super::select_direct_ids;

    // Ranked: s1 (0.9), s0 (0.3). min_prob 0.5 picks only s1 first; the
    // second pass fills s0 to reach min_tools = 2.
    let ranking = vec![(1u64, 0.9f32), (0u64, 0.3f32)];
    let picks = select_direct_ids(&ranking, 0.5, 2, 6);
    assert_eq!(picks, std::collections::HashSet::from([0usize, 1usize]));

    // Without the fill (min_tools = 1) only s1 clears the threshold.
    let picks = select_direct_ids(&ranking, 0.5, 1, 6);
    assert_eq!(picks, std::collections::HashSet::from([1usize]));

    // max_tools caps the selection.
    let ranking = vec![(0u64, 0.9f32), (1u64, 0.8f32), (2u64, 0.7f32)];
    let picks = select_direct_ids(&ranking, 0.1, 1, 2);
    assert_eq!(picks.len(), 2);
}

#[test]
fn strategy_weights_move_tools_across_threshold() {
    use super::apply_strategy_weights;
    use super::select_direct_ids;
    use crate::session::strategy_runtime::StrategyTurnContext;

    // Jev ranks s0 (favored) at 0.9 and s1 at 0.1. With weight 0.1 on s0 the
    // renormalized score drops below the 0.5 threshold, so only s1 is picked
    // when min_tools = 1 — proving selection consumes the adjusted ranking.
    let pool_keys = vec!["srv/favored".to_string(), "srv/plain".to_string()];
    let ranking = vec![(0u64, 0.9f32), (1u64, 0.1f32)];
    let mut weights = std::collections::HashMap::new();
    weights.insert("srv/favored".to_string(), 0.1f32);
    let strategy = StrategyTurnContext {
        name: "s".into(),
        node: "n".into(),
        guidance: String::new(),
        tool_weights: weights,
        strong_prior_tools: Vec::new(),
    };
    let adjusted = apply_strategy_weights(ranking, &pool_keys, Some(&strategy));
    let picks = select_direct_ids(&adjusted, 0.5, 1, 6);
    assert_eq!(
        picks,
        std::collections::HashSet::from([1usize]),
        "selection uses the strategy-adjusted probabilities: {adjusted:?}"
    );
}

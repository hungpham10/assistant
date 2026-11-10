//! Tests cho `jev_suggest` — phần thuần (mapping, fail-open) chạy offline.

use super::JevOutcome;
use super::build_planning_state;
use super::call_jev_planning;
use super::last_user_question;
use super::parse_jev_reply;
use super::suggest_direct_mcp_tools;
use super::tool_label;
use super::truncate_chars;
use crate::mcp_tool_exposure::McpToolExposure;
use codegraph_core::JevConfig;
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
    )
    .await;
    assert!(out.is_none());

    // Enabled nhưng thiếu token env → vẫn fail-open.
    let cfg = ToolSuggestJevConfig {
        enabled: true,
        api_key_env: Some("CODEX_TEST_JEV_SUGGEST_ABSENT_KEY".into()),
        ..ToolSuggestJevConfig::default()
    };
    let out = suggest_direct_mcp_tools("run a shell command", None, exposure, &cfg).await;
    assert!(out.is_none());
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
    let out = suggest_direct_mcp_tools("anything", None, exposure, &cfg).await;
    assert!(out.is_none());
}

#[test]
fn build_planning_state_includes_guidance() {
    let state = build_planning_state("run tests", Some("my-repo"), 5);
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
    let state = build_planning_state("run tests", None, 3);
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
    let result =
        suggest_direct_mcp_tools("run the tests", Some("test-repo".into()), exposure, &cfg).await;

    // Should fail-open because test key doesn't exist
    assert!(result.is_none());
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

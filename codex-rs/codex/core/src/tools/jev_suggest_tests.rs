//! Tests cho `jev_suggest` — phần thuần (mapping, fail-open) chạy offline.

use super::last_user_question;
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

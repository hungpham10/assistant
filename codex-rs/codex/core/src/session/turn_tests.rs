use super::*;
use codex_extension_api::ExtensionData;
use codex_extension_api::TurnItemContributor;
use codex_protocol::items::AgentMessageContent;
use pretty_assertions::assert_eq;
use std::sync::Arc;

struct RewriteAgentMessageContributor;

impl TurnItemContributor for RewriteAgentMessageContributor {
    fn contribute<'a>(
        &'a self,
        _thread_store: &'a ExtensionData,
        _turn_store: &'a ExtensionData,
        item: &'a mut TurnItem,
    ) -> codex_extension_api::ExtensionFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if let TurnItem::AgentMessage(agent_message) = item {
                agent_message.content = vec![AgentMessageContent::Text {
                    text: "plan contributed assistant text".to_string(),
                }];
            }
            Ok(())
        })
    }
}

fn assistant_output_text(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: Some("msg-1".to_string()),
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[tokio::test]
async fn plan_mode_uses_contributed_turn_item_for_last_agent_message() {
    let (mut session, turn_context) = crate::session::tests::make_session_and_context().await;
    let mut builder = codex_extension_api::ExtensionRegistryBuilder::new();
    builder.turn_item_contributor(Arc::new(RewriteAgentMessageContributor));
    session.services.extensions = Arc::new(builder.build());
    let turn_store = ExtensionData::new(turn_context.sub_id.clone());
    let mut state = PlanModeStreamState::new(&turn_context.sub_id);
    let mut last_agent_message = None;
    let item = assistant_output_text("original assistant text");

    let handled = handle_assistant_item_done_in_plan_mode(
        &session,
        &turn_context,
        &turn_store,
        &item,
        &mut state,
        /*previously_active_item*/ None,
        &mut last_agent_message,
    )
    .await;

    assert!(handled);
    assert_eq!(
        last_agent_message.as_deref(),
        Some("plan contributed assistant text")
    );
}

#[test]
fn recent_tool_calls_keys_and_order() {
    let items = vec![
        ResponseItem::FunctionCall {
            id: None,
            name: "codegraph_flow".into(),
            namespace: Some("codegraph".into()),
            arguments: "{}".into(),
            call_id: "c1".into(),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::CustomToolCall {
            id: None,
            status: None,
            call_id: "c2".into(),
            name: "shell".into(),
            namespace: None,
            input: "{}".into(),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::CustomToolCall {
            id: None,
            status: None,
            call_id: "c3".into(),
            name: "shell".into(),
            namespace: None,
            input: "{}".into(),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::Message {
            id: None,
            role: "user".into(),
            phase: None,
            internal_chat_message_metadata_passthrough: None,
            content: vec![],
        },
        ResponseItem::FunctionCall {
            id: None,
            name: "apply_patch".into(),
            namespace: None,
            arguments: "{}".into(),
            call_id: "c4".into(),
            internal_chat_message_metadata_passthrough: None,
        },
    ];

    assert_eq!(
        recent_tool_calls(&items),
        vec![
            "codegraph/codegraph_flow".to_string(),
            "shell".to_string(),
            "apply_patch".to_string(),
        ]
    );
}

#[test]
fn recent_tool_calls_caps_at_limit_most_recent_first() {
    let items: Vec<ResponseItem> = (0..20)
        .map(|i| ResponseItem::FunctionCall {
            id: None,
            name: format!("tool_{i}"),
            namespace: None,
            arguments: "{}".into(),
            call_id: format!("c{i}"),
            internal_chat_message_metadata_passthrough: None,
        })
        .collect();

    let keys = recent_tool_calls(&items);
    assert_eq!(keys.len(), super::MAX_RECENT_TOOL_CALLS);
    // Most recent last.
    assert_eq!(keys.last().map(String::as_str), Some("tool_19"));
    assert_eq!(keys.first().map(String::as_str), Some("tool_12"));
}

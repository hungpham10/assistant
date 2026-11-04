use crate::session::TurnInput;
use crate::session::turn_context::TurnContext;
use crate::tools::families::build_family_registry;
use codex_pipeline::command::{AtCommandParser, CommandParser};
use codex_pipeline::executor::run_chain;
use codex_protocol::models::ResponseItem;
use codex_protocol::user_input::UserInput;
use std::sync::Arc;

pub(crate) async fn run_command_plane(
    _turn_context: &Arc<TurnContext>,
    input: &[TurnInput],
) -> Vec<ResponseItem> {
    let registry = build_family_registry();
    let mut items = Vec::new();

    for turn_input in input {
        if let TurnInput::UserInput { content, .. } = turn_input {
            for item in content {
                if let UserInput::Text { text, .. } = item {
                    if text.is_empty() {
                        continue;
                    }
                    let parser = AtCommandParser;
                    let plan = parser.parse(text);
                    if plan.commands.is_empty() {
                        continue;
                    }
                    let outcome = run_chain(&registry, &plan).await;
                    for result in outcome.results {
                        let json = serde_json::to_string_pretty(&result.output)
                            .unwrap_or_else(|_| result.output.to_string());
                        items.push(ResponseItem::Message {
                            id: None,
                            role: "user".to_string(),
                            content: vec![codex_protocol::models::ContentItem::InputText { text: json }],
                            phase: None,
                            internal_chat_message_metadata_passthrough: None,
                        });
                    }
                    if !outcome.hints.is_empty() {
                        let hints = outcome.hints.join("\n");
                        items.push(ResponseItem::Message {
                            id: None,
                            role: "user".to_string(),
                            content: vec![codex_protocol::models::ContentItem::InputText {
                                text: hints,
                            }],
                            phase: None,
                            internal_chat_message_metadata_passthrough: None,
                        });
                    }
                }
            }
        }
    }
    items
}

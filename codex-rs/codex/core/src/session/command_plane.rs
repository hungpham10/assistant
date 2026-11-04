use crate::session::TurnInput;
use crate::session::turn_context::TurnContext;
use crate::tools::families::build_family_registry;
use codex_enrich::command::AtCommandParser;
use codex_enrich::command::CommandParser;
use codex_enrich::executor::run_chain;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::user_input::UserInput;
use std::sync::Arc;

pub(crate) async fn run_command_plane(
    turn_context: &Arc<TurnContext>,
    input: &[TurnInput],
) -> Vec<ResponseItem> {
    let registry = build_family_registry();
    let mut items = Vec::new();
    let pipeline_cfg = &turn_context.config.pipeline;
    let token_budget = pipeline_cfg.token_budget;
    let max_ratio = pipeline_cfg.max_total_context_ratio as u64;

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
                    let mut items_for_budget: Vec<ResponseItem> = Vec::new();
                    for result in outcome.results {
                        let json = serde_json::to_string_pretty(&result.output)
                            .unwrap_or_else(|_| result.output.to_string());
                        items_for_budget.push(ResponseItem::Message {
                            id: None,
                            role: "user".to_string(),
                            content: vec![ContentItem::InputText { text: json }],
                            phase: None,
                            internal_chat_message_metadata_passthrough: None,
                        });
                    }
                    codex_tools::truncate_assistant_output_text_to_token_budget(
                        &mut items_for_budget,
                        token_budget,
                    );

                    // Check total context ratio if possible
                    if let Some(ctx_window) = turn_context.model_context_window() {
                        let mut total_tokens: u64 = 0;
                        for it in &items_for_budget {
                            if let ResponseItem::Message { content, .. } = it {
                                for c in content {
                                    if let ContentItem::InputText { text } = c {
                                        total_tokens += (text.len() as u64) / 4;
                                    }
                                }
                            }
                        }
                        let limit = (ctx_window as u64) * max_ratio / 100;
                        if total_tokens > limit && !items_for_budget.is_empty() {
                            // Drop enrichment on exceed (fail-open, keep prompt)
                            items_for_budget.clear();
                        }
                    }

                    items.extend(items_for_budget);
                    if !outcome.hints.is_empty() {
                        let hints = outcome.hints.join("\n");
                        items.push(ResponseItem::Message {
                            id: None,
                            role: "user".to_string(),
                            content: vec![ContentItem::InputText { text: hints }],
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

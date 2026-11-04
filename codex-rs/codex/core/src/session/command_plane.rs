use crate::session::TurnInput;
use crate::session::turn_context::TurnContext;
use crate::tools::families::build_family_registry;
use codex_pipeline::command::AtCommandParser;
use codex_pipeline::command::CommandParser;
use codex_pipeline::executor::run_chain;
use codex_protocol::models::ResponseItem;
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
                if let codex_protocol::user_input::UserInput::Text { text, .. } = item {
                    let parser = AtCommandParser;
                    let plan = parser.parse(text);
                    if !plan.commands.is_empty() {
                        let outcome = run_chain(&registry, &plan).await;
                        for result in outcome.results {
                            let json = result.output.to_string();
                            items.push(ResponseItem::Thought {
                                id: None,
                                thought: json,
                            });
                        }
                    }
                }
            }
        }
    }
    items
}

use crate::session::TurnInput;
use crate::session::turn_context::TurnContext;
use crate::tools::families::build_family_registry;
use codex_pipeline::component::StreamMessage;
use codex_protocol::models::ResponseItem;
use std::sync::Arc;

pub(crate) async fn run_command_plane(
    _turn_context: &Arc<TurnContext>,
    input: &[TurnInput],
) -> Vec<ResponseItem> {
    let registry = build_family_registry();
    let _ = input;
    // Build stream message and run components from registered families
    let _msg = StreamMessage::default();
    for family in registry.families() {
        let _components = family.build_components(&serde_json::json!({}));
        // Components would be executed here with taps/token budget
    }
    Vec::new()
}

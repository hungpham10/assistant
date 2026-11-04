use crate::session::TurnInput;
use crate::session::turn_context::TurnContext;
use crate::tools::families::build_family_registry;
use codex_protocol::models::ResponseItem;
use std::sync::Arc;

pub(crate) async fn run_command_plane(
    _turn_context: &Arc<TurnContext>,
    input: &[TurnInput],
) -> Vec<ResponseItem> {
    let _registry = build_family_registry();
    let _ = input;
    Vec::new()
}

use serde_json::Value;
use serde_json::json;

use crate::client_common::ForcedTool;

/// OpenAI chat wire `tool_choice` for a request: `"auto"` by default, or an
/// explicit function forcing the provider to call the named tool (Jev `force`
/// enforcement). MCP tool keys arrive in the `server/tool` tracker format, so
/// the namespace is carried alongside the callable name when present.
pub(crate) fn openai_chat_tool_choice(forced_tool: Option<&ForcedTool>) -> Value {
    match forced_tool {
        Some(forced) => {
            let mut function = json!({ "name": forced.name });
            if let Some(namespace) = &forced.namespace {
                function["namespace"] = json!(namespace);
            }
            json!({ "type": "function", "function": function })
        }
        None => json!("auto"),
    }
}

/// Anthropic wire `tool_choice`: `{"type": "auto"}` by default, or
/// `{"type": "tool", "name": ...}` to force the named tool. The Anthropic wire
/// has no namespace concept, so only the callable name is sent.
pub(crate) fn anthropic_tool_choice(forced_tool: Option<&ForcedTool>) -> Value {
    match forced_tool {
        Some(forced) => json!({ "type": "tool", "name": forced.name }),
        None => json!({ "type": "auto" }),
    }
}

#[cfg(test)]
#[path = "tool_choice_tests.rs"]
mod tests;

use pretty_assertions::assert_eq;
use serde_json::json;

use super::anthropic_tool_choice;
use super::openai_chat_tool_choice;
use crate::client_common::ForcedTool;

#[test]
fn openai_chat_tool_choice_defaults_to_auto() {
    assert_eq!(openai_chat_tool_choice(None), json!("auto"));
}

#[test]
fn openai_chat_tool_choice_forces_function_with_namespace() {
    assert_eq!(
        openai_chat_tool_choice(Some(&ForcedTool {
            namespace: Some("finhay".to_string()),
            name: "finhay_get_quote".to_string(),
        })),
        json!({
            "type": "function",
            "function": {
                "name": "finhay_get_quote",
                "namespace": "finhay",
            }
        })
    );
}

#[test]
fn openai_chat_tool_choice_forces_function_without_namespace() {
    assert_eq!(
        openai_chat_tool_choice(Some(&ForcedTool {
            namespace: None,
            name: "shell".to_string(),
        })),
        json!({
            "type": "function",
            "function": {
                "name": "shell",
            }
        })
    );
}

#[test]
fn anthropic_tool_choice_defaults_to_auto() {
    assert_eq!(anthropic_tool_choice(None), json!({ "type": "auto" }));
}

#[test]
fn anthropic_tool_choice_forces_named_tool() {
    assert_eq!(
        anthropic_tool_choice(Some(&ForcedTool {
            namespace: Some("finhay".to_string()),
            name: "finhay_get_quote".to_string(),
        })),
        json!({ "type": "tool", "name": "finhay_get_quote" })
    );
}

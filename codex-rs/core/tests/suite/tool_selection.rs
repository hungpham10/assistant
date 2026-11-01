#![cfg(not(target_os = "windows"))]
#![allow(clippy::unwrap_used)]

use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;

fn tool_names(body: &Value) -> Vec<String> {
    body["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

async fn run_plain_turn(test: &TestCodex, prompt: &str) -> anyhow::Result<()> {
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: prompt.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    Ok(())
}

async fn run_turn_in_mode(
    test: &TestCodex,
    prompt: &str,
    mode: codex_protocol::config_types::ModeKind,
) -> anyhow::Result<()> {
    let session_model = test.session_configured.model.clone();
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: prompt.into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                    mode,
                    settings: codex_protocol::config_types::Settings {
                        model: session_model,
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            },
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_tool_enabled_hides_tool_from_model_but_keeps_others() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let _first = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "before toggle"),
            ev_completed("resp-1"),
        ]),
    )
    .await;
    let second = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-2"),
            ev_assistant_message("msg-2", "after toggle"),
            ev_completed("resp-2"),
        ]),
    )
    .await;

    let mut test = test_codex();
    let test = test.build(&server).await?;

    run_plain_turn(&test, "before toggle").await?;
    test.codex
        .submit(Op::SetToolEnabled {
            name: "apply_patch".to_string(),
            enabled: false,
        })
        .await?;
    run_plain_turn(&test, "after toggle").await?;

    let before = tool_names(&_first.single_request().body_json());
    let after = tool_names(&second.single_request().body_json());

    assert!(
        before.iter().any(|name| name == "apply_patch"),
        "apply_patch should be advertised before the toggle; tools: {before:?}"
    );
    assert!(
        !after.iter().any(|name| name == "apply_patch"),
        "apply_patch should be hidden after the toggle; tools: {after:?}"
    );
    assert!(
        after.iter().any(|name| name == "update_plan"),
        "protected tools must stay advertised; tools: {after:?}"
    );
    assert_eq!(after.len() + 1, before.len());

    // Re-enable: the tool must come back on the next request.
    test.codex
        .submit(Op::SetToolEnabled {
            name: "apply_patch".to_string(),
            enabled: true,
        })
        .await?;
    let third = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-3"),
            ev_assistant_message("msg-3", "re-enabled"),
            ev_completed("resp-3"),
        ]),
    )
    .await;
    run_plain_turn(&test, "re-enabled").await?;
    let restored = tool_names(&third.single_request().body_json());
    assert!(
        restored.iter().any(|name| name == "apply_patch"),
        "apply_patch should be advertised again after re-enable; tools: {restored:?}"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_namespace_wildcard_keeps_builtin_tools() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let _first = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "hi"),
            ev_completed("resp-1"),
        ]),
    )
    .await;
    let second = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-2"),
            ev_assistant_message("msg-2", "hi again"),
            ev_completed("resp-2"),
        ]),
    )
    .await;

    let mut test = test_codex();
    let test = test.build(&server).await?;
    run_plain_turn(&test, "hi").await?;
    let before = tool_names(&_first.single_request().body_json());

    test.codex
        .submit(Op::SetToolEnabled {
            name: "mcp-server-that-is-not-connected:*".to_string(),
            enabled: false,
        })
        .await?;
    run_plain_turn(&test, "hi again").await?;
    let after = tool_names(&second.single_request().body_json());
    assert_eq!(before, after);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plan_mode_hides_write_tools() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let request = mount_sse_sequence(
        &server,
        vec![sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "planning"),
            ev_completed("resp-1"),
        ])],
    )
    .await;

    let mut test = test_codex();
    let test = test.build(&server).await?;
    run_turn_in_mode(
        &test,
        "plan it",
        codex_protocol::config_types::ModeKind::Plan,
    )
    .await?;

    let body = request.single_request().body_json();
    let tools = tool_names(&body);
    assert!(
        !tools.iter().any(|name| name == "apply_patch"),
        "Plan mode should not advertise apply_patch; tools: {tools:?}"
    );
    assert!(
        tools.iter().any(|name| name == "update_plan"),
        "Plan mode must keep update_plan; tools: {tools:?}"
    );

    Ok(())
}

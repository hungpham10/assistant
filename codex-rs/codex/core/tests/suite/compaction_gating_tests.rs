use anyhow::Result;
use codex_core::CodexThread;
use codex_core::StartThreadOptions;
use codex_core::compact::SUMMARIZATION_PROMPT;
use codex_core::config::Config;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::built_in_model_providers;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ResponseMock;
use core_test_support::responses::ResponsesRequest;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed_with_tokens;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use std::sync::Arc;
use wiremock::MockServer;

const GUARDIAN_REVIEWER_NAME: &str = "guardian";
const FIRST_MSG: &str = "first turn prompt";
const SECOND_MSG: &str = "second turn prompt";
const AUTO_SUMMARY_TEXT: &str = "AUTO_COMPACT_SUMMARY";

fn set_test_compact_prompt(config: &mut Config) {
    config.compact_prompt = Some(SUMMARIZATION_PROMPT.to_string());
}

fn non_openai_model_provider(server: &MockServer) -> ModelProviderInfo {
    let mut provider = built_in_model_providers(/*openai_base_url*/ None)["openai"].clone();
    provider.name = "OpenAI (test)".into();
    provider.base_url = Some(format!("{}/v1", server.uri()));
    provider.supports_websockets = false;
    provider
}

fn user_input(text: &str) -> Op {
    Op::UserInput {
        items: vec![UserInput::Text {
            text: text.to_string(),
            text_elements: Vec::new(),
        }],
        final_output_json_schema: None,
        responsesapi_client_metadata: None,
        additional_context: Default::default(),
        thread_settings: Default::default(),
    }
}

fn sse_turn(id: &str, message: &str, total_tokens: i64) -> String {
    sse(vec![
        ev_response_created(id),
        ev_assistant_message(id, message),
        ev_completed_with_tokens(id, total_tokens),
    ])
}

/// Starts a thread with the given session source using a token limit low enough that pre-turn
/// compaction triggers before the second turn (the first response reports usage over the limit).
async fn build_thread_with_source(
    server: &MockServer,
    session_source: SessionSource,
) -> Result<Arc<CodexThread>> {
    let model_provider = non_openai_model_provider(server);
    let mut builder = test_codex().with_config(move |config| {
        config.model_provider = model_provider;
        set_test_compact_prompt(config);
        config.model_auto_compact_token_limit = Some(200_000);
    });
    let test = builder.build(server).await?;
    let thread = test
        .thread_manager
        .start_thread_with_options(StartThreadOptions {
            config: test.config.clone(),
            allow_provider_model_fallback: false,
            initial_history: InitialHistory::New,
            history_mode: None,
            session_source: Some(session_source),
            thread_source: None,
            dynamic_tools: Vec::new(),
            metrics_service_name: None,
            parent_trace: None,
            environments: Vec::new(),
            thread_extension_init: Default::default(),
            supports_openai_form_elicitation: false,
        })
        .await?;
    Ok(thread.thread)
}

async fn run_two_turns(thread: &CodexThread) {
    thread
        .submit(user_input(FIRST_MSG))
        .await
        .expect("first turn submission");
    wait_for_event(thread, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
    thread
        .submit(user_input(SECOND_MSG))
        .await
        .expect("second turn submission");
    wait_for_event(thread, |ev| matches!(ev, EventMsg::TurnComplete(_))).await;
}

fn request_contains(req: &ResponsesRequest, needle: &str) -> bool {
    req.body_contains_text(needle)
}

#[tokio::test]
async fn pre_turn_compaction_runs_for_regular_session() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    let requests = mount_sse_sequence(
        &server,
        vec![
            sse_turn("resp-1", "first reply", 250_000),
            // Pre-turn compaction of turn 2 runs locally against this request.
            sse_turn("resp-compact", AUTO_SUMMARY_TEXT, 300),
            sse_turn("resp-2", "second reply", 120),
        ],
    )
    .await;

    let thread = build_thread_with_source(&server, SessionSource::Cli)
        .await
        .unwrap();
    run_two_turns(&thread).await;

    let captured: Vec<ResponsesRequest> = requests.requests();
    assert_eq!(
        captured.len(),
        3,
        "expected turn, compaction, turn requests"
    );
    assert!(
        captured
            .iter()
            .any(|req| request_contains(req, AUTO_SUMMARY_TEXT)),
        "compaction request should contain the compact prompt"
    );
}

#[tokio::test]
async fn pre_turn_compaction_skipped_for_guardian_reviewer() {
    skip_if_no_network!();

    let server = start_mock_server().await;
    // No compaction request is mounted: guardian reviewer sessions must skip pre-turn
    // compaction, so only the two regular turn requests should hit the server.
    let requests: ResponseMock = mount_sse_sequence(
        &server,
        vec![
            sse_turn("resp-1", "first reply", 250_000),
            sse_turn("resp-2", "second reply", 120),
        ],
    )
    .await;

    let thread = build_thread_with_source(
        &server,
        SessionSource::SubAgent(SubAgentSource::Other(GUARDIAN_REVIEWER_NAME.to_string())),
    )
    .await
    .unwrap();
    run_two_turns(&thread).await;

    let captured: Vec<ResponsesRequest> = requests.requests();
    assert_eq!(
        captured.len(),
        2,
        "guardian reviewer sessions must not issue a pre-turn compaction request"
    );
}

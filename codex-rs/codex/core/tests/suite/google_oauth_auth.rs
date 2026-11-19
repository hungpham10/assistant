//! Integration tests for Google OAuth fallback auth on the `google` wire API.
//!
//! The google route must prefer an API key and fall back to stored Google
//! OAuth tokens (`codex login --google`), sending them as a Bearer header.

use std::path::Path;
use std::sync::Arc;

use codex_config::types::AuthCredentialsStoreMode;
use codex_core::CodexResponsesMetadata;
use codex_core::ModelClient;
use codex_core::Prompt;
use codex_core::ResponseEvent;
use codex_features::Feature;
use codex_login::AuthKeyringBackendKind;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::GoogleTokenData;
use codex_login::auth::AgentIdentityAuthPolicy;
use codex_login::save_auth;
use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_otel::SessionTelemetry;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::error::CodexErr;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::SessionSource;
use codex_rollout_trace::InferenceTraceContext;
use core_test_support::TestCodexResponsesRequestKind;
use core_test_support::load_default_config_for_test;
use core_test_support::responses_metadata as test_responses_metadata;
use futures::StreamExt;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_string_contains;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::path_regex;
use wiremock::matchers::query_param;

const TEST_INSTALLATION_ID: &str = "11111111-1111-4111-8111-111111111111";
const TEST_WINDOW_ID: &str = "test-thread:0";

// Serializes tests that override the global `CODE_ASSIST_ENDPOINT` env var.
static CODE_ASSIST_ENDPOINT_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
    std::sync::OnceLock::new();
fn code_assist_endpoint_lock() -> &'static std::sync::Mutex<()> {
    CODE_ASSIST_ENDPOINT_LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

fn google_sse_body(text: &str) -> String {
    format!(
        "data: {{\"candidates\":[{{\"content\":{{\"role\":\"model\",\"parts\":[{{\"text\":\"{text}\"}}]}}}}]}}\n\n"
    )
}

fn google_tokens(access_token: &str, refresh_token: &str, expires_at: i64) -> GoogleTokenData {
    GoogleTokenData {
        access_token: access_token.to_string(),
        refresh_token: refresh_token.to_string(),
        expires_at,
        email: None,
        last_refresh: 0,
        project_id: None,
    }
}

/// Persists Google OAuth tokens into `$CODEX_HOME/auth.json`.
fn seed_google_tokens(codex_home: &Path, tokens: GoogleTokenData) {
    let auth_dot_json = codex_login::AuthDotJson {
        google_oauth: Some(tokens),
        ..codex_login::AuthDotJson::default()
    };
    save_auth(
        codex_home,
        &auth_dot_json,
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .expect("seed google oauth tokens");
}

async fn auth_manager_for_home(codex_home: &Path) -> Arc<AuthManager> {
    Arc::new(
        AuthManager::new(
            codex_home.to_path_buf(),
            /*enable_codex_api_key_env*/ false,
            AuthCredentialsStoreMode::File,
            /*forced_chatgpt_workspace_id*/ None,
            /*chatgpt_base_url*/ None,
            AuthKeyringBackendKind::default(),
            /*auth_route_config*/ None,
        )
        .await,
    )
}

fn google_provider(server: &MockServer) -> ModelProviderInfo {
    ModelProviderInfo {
        name: "google-oauth-test".into(),
        base_url: Some(server.uri()),
        env_key: None,
        env_key_instructions: None,
        experimental_bearer_token: None,
        auth: None,
        aws: None,
        wire_api: WireApi::Google,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: Some(0),
        stream_max_retries: Some(0),
        stream_idle_timeout_ms: Some(5_000),
        websocket_connect_timeout_ms: None,
        requires_openai_auth: false,
        supports_websockets: false,
        supports_tool_calling: None,
        supports_parallel_tool_calls: None,
    }
}

struct TestClient {
    client: ModelClient,
    thread_id: ThreadId,
    model_info: ModelInfo,
    responses_metadata: CodexResponsesMetadata,
    effort: Option<ReasoningEffort>,
    summary: Option<ReasoningSummary>,
}

#[expect(clippy::unwrap_used)]
async fn build_test_client(auth_manager: Arc<AuthManager>, server: &MockServer) -> TestClient {
    let provider = google_provider(server);
    let codex_home = TempDir::new().unwrap();
    let mut config = load_default_config_for_test(&codex_home).await;
    config.model_provider_id = provider.name.clone();
    config.model_provider = provider.clone();
    let effort = config.model_reasoning_effort.clone();
    let summary = config.model_reasoning_summary;
    let model = codex_core::test_support::get_model_offline(config.model.as_deref());
    config.model = Some(model.clone());
    let config = Arc::new(config);
    let model_info =
        codex_core::test_support::construct_model_info_offline(model.as_str(), &config);
    let thread_id = ThreadId::new();
    let thread_id_str = thread_id.to_string();
    let responses_metadata = test_responses_metadata(
        TEST_INSTALLATION_ID,
        &thread_id_str,
        &thread_id_str,
        /*turn_id*/ None,
        TEST_WINDOW_ID.to_string(),
        &SessionSource::Exec,
        /*parent_thread_id*/ None,
        TestCodexResponsesRequestKind::Turn,
    );
    let client = ModelClient::new(
        Some(auth_manager),
        AgentIdentityAuthPolicy::JwtOnly,
        thread_id,
        provider,
        SessionSource::Exec,
        "test_originator".to_string(),
        config.model_verbosity,
        /*enable_request_compression*/ false,
        /*include_timing_metrics*/ false,
        /*beta_features_header*/ None,
        codex_tools::Harness::Native,
        /*harness_guidance*/ true,
        /*item_ids_enabled*/ config.features.enabled(Feature::ItemIds),
        /*concurrent_reasoning_summaries_enabled*/
        config
            .features
            .enabled(Feature::ConcurrentReasoningSummaries),
        /*attestation_provider*/ None,
        config.http_client_factory(),
    );
    TestClient {
        client,
        thread_id,
        model_info,
        responses_metadata,
        effort,
        summary,
    }
}

async fn run_one_turn(test_client: TestClient) {
    let TestClient {
        client,
        thread_id,
        model_info,
        responses_metadata,
        effort,
        summary,
    } = test_client;
    let session_telemetry = SessionTelemetry::new(
        thread_id,
        model_info.slug.as_str(),
        model_info.slug.as_str(),
        /*account_id*/ None,
        Some("test@test.com".to_string()),
        /*auth_mode*/ None,
        "test_originator".to_string(),
        /*log_user_prompts*/ false,
        "test".to_string(),
        SessionSource::Exec,
    );
    let mut client_session = client.new_session();
    let mut prompt = Prompt::default();
    prompt.input.push(ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "hello".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    });

    let mut stream = client_session
        .stream(
            &prompt,
            &model_info,
            &session_telemetry,
            effort,
            summary.unwrap_or(ReasoningSummary::Auto),
            /*service_tier*/ None,
            &responses_metadata,
            &InferenceTraceContext::disabled(),
        )
        .await
        .expect("google stream to start");

    while let Some(event) = stream.next().await {
        if let Ok(ResponseEvent::Completed { .. }) = event {
            return;
        }
    }
    panic!("expected a Completed event from the google stream");
}

/// Mounts Code Assist setup (`loadCodeAssist`) returning a cached project, so
/// onboarding is not triggered.
async fn mount_code_assist_setup(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1internal:loadCodeAssist"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "currentTier": { "id": "free-tier", "name": "Free", "isDefault": true },
            "cloudaicompanionProject": "proj-123",
        })))
        .expect(1)
        .mount(server)
        .await;
}

/// Mounts the Code Assist streaming endpoint expecting the given Bearer token.
/// The body is a Code Assist envelope `{ "response": {...} }`.
async fn mount_code_assist_sse(server: &MockServer, bearer_token: &str, text: &str) {
    Mock::given(method("POST"))
        .and(path("/v1internal:streamGenerateContent"))
        .and(query_param("alt", "sse"))
        .and(header("authorization", format!("Bearer {bearer_token}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(code_assist_sse_body(text)),
        )
        .expect(1)
        .mount(server)
        .await;
}

fn code_assist_sse_body(text: &str) -> String {
    format!(
        "data: {{\"response\":{{\"candidates\":[{{\"content\":{{\"role\":\"model\",\"parts\":[{{\"text\":\"{text}\"}}]}}}}]}}}}\n\n"
    )
}

#[allow(clippy::await_holding_lock)] // intentional: serializes env-var mutation across the whole test
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn google_wire_uses_oauth_bearer_token_without_api_key() {
    let server = MockServer::start().await;
    // Route the free Code Assist flow at the mock server. The endpoint lock
    // serializes tests that override the global CODE_ASSIST_ENDPOINT env var.
    let _lock = code_assist_endpoint_lock().lock().unwrap();
    // SAFETY(test): serialized via CODE_ASSIST_ENDPOINT_LOCK; restored below.
    unsafe { std::env::set_var("CODE_ASSIST_ENDPOINT", server.uri()) };
    mount_code_assist_setup(&server).await;
    mount_code_assist_sse(&server, "google-access-token", "hi there").await;

    let codex_home = TempDir::new().unwrap();
    seed_google_tokens(
        codex_home.path(),
        google_tokens(
            "google-access-token",
            "google-refresh-token",
            i64::MAX / 2, // far from expiry
        ),
    );
    let auth_manager = auth_manager_for_home(codex_home.path()).await;

    let test_client = build_test_client(auth_manager, &server).await;
    run_one_turn(test_client).await;
    server.verify().await;
    // SAFETY(test): serialized via CODE_ASSIST_ENDPOINT_LOCK.
    unsafe { std::env::remove_var("CODE_ASSIST_ENDPOINT") };
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn google_wire_api_key_auth_takes_precedence_over_oauth_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex("v1beta/models/.+:streamGenerateContent"))
        .and(header("x-goog-api-key", "codex-auth-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(google_sse_body("hi")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let codex_home = TempDir::new().unwrap();
    seed_google_tokens(
        codex_home.path(),
        google_tokens("google-access-token", "google-refresh-token", i64::MAX / 2),
    );
    let auth_manager = AuthManager::from_auth_for_testing_with_home(
        CodexAuth::from_api_key("codex-auth-key"),
        codex_home.path().to_path_buf(),
    );

    let test_client = build_test_client(auth_manager, &server).await;
    run_one_turn(test_client).await;
    server.verify().await;
}

#[allow(clippy::await_holding_lock)] // intentional: serializes env-var mutation across the whole test
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn google_wire_refreshes_expired_oauth_tokens_and_persists_them() {
    let server = MockServer::start().await;
    let _lock = code_assist_endpoint_lock().lock().unwrap();
    // Point Google token refresh at the mock server for the duration of the test.
    let token_url = format!("{}/token", server.uri());
    // Route the free Code Assist flow at the mock server too.
    // SAFETY(test): serialized via CODE_ASSIST_ENDPOINT_LOCK; restored below.
    unsafe { std::env::set_var("CODE_ASSIST_ENDPOINT", server.uri()) };
    // SAFETY(test): serialized with respect to other google tests; restored below.
    unsafe { std::env::set_var("CODEX_GOOGLE_OAUTH_TOKEN_URL", &token_url) };

    // Token refresh endpoint hands out a fresh access token.
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=google-refresh-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "refreshed-access-token",
            "expires_in": 3600,
        })))
        .expect(1)
        .mount(&server)
        .await;
    // The Code Assist generateContent call must carry the refreshed token.
    mount_code_assist_setup(&server).await;
    mount_code_assist_sse(&server, "refreshed-access-token", "hi").await;

    let codex_home = TempDir::new().unwrap();
    seed_google_tokens(
        codex_home.path(),
        // Expired access token forces a refresh before the request.
        google_tokens("expired-access-token", "google-refresh-token", 0),
    );
    let auth_manager = auth_manager_for_home(codex_home.path()).await;

    let test_client = build_test_client(auth_manager, &server).await;
    run_one_turn(test_client).await;
    server.verify().await;

    // Refreshed tokens must be persisted back to auth.json.
    let stored = codex_login::load_google_oauth_tokens(
        codex_home.path(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .expect("load persisted google tokens")
    .expect("google tokens persisted");
    assert_eq!(stored.access_token, "refreshed-access-token");
    assert!(stored.expires_at > 0);
    assert_eq!(stored.refresh_token, "google-refresh-token");

    unsafe { std::env::remove_var("CODEX_GOOGLE_OAUTH_TOKEN_URL") };
    // SAFETY(test): serialized via CODE_ASSIST_ENDPOINT_LOCK.
    unsafe { std::env::remove_var("CODE_ASSIST_ENDPOINT") };
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn google_wire_errors_without_api_key_or_oauth_tokens() {
    let server = MockServer::start().await;
    let codex_home = TempDir::new().unwrap();
    let auth_manager = auth_manager_for_home(codex_home.path()).await;

    let test_client = build_test_client(auth_manager, &server).await;
    let TestClient {
        client,
        thread_id,
        model_info,
        responses_metadata,
        effort,
        summary,
    } = test_client;
    let session_telemetry = SessionTelemetry::new(
        thread_id,
        model_info.slug.as_str(),
        model_info.slug.as_str(),
        /*account_id*/ None,
        Some("test@test.com".to_string()),
        /*auth_mode*/ None,
        "test_originator".to_string(),
        /*log_user_prompts*/ false,
        "test".to_string(),
        SessionSource::Exec,
    );
    let mut client_session = client.new_session();
    let mut prompt = Prompt::default();
    prompt.input.push(ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "hello".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    });
    let err = client_session
        .stream(
            &prompt,
            &model_info,
            &session_telemetry,
            effort,
            summary.unwrap_or(ReasoningSummary::Auto),
            /*service_tier*/ None,
            &responses_metadata,
            &InferenceTraceContext::disabled(),
        )
        .await;
    let err = match err {
        Ok(_stream) => panic!("expected a missing-auth error, got a stream"),
        Err(err) => err,
    };
    match err {
        CodexErr::InvalidRequest(message) => {
            assert!(message.contains("codex login --google"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "no request should reach the mock server without credentials"
    );
}

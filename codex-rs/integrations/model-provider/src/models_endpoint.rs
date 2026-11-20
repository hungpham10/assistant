use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use codex_api::AgentIdentityTelemetry;
use codex_api::GoogleGenerateContentClient as ApiGoogleGenerateContentClient;
use codex_api::GoogleModel;
use codex_api::ModelsClient;
use codex_api::RequestTelemetry;
use codex_api::ReqwestTransport;
use codex_api::SharedAuthProvider;
use codex_api::TransportError;
use codex_api::auth_header_telemetry;
use codex_api::google_models_to_model_infos;
use codex_api::map_api_error;
use codex_feedback::FeedbackRequestTags;
use codex_feedback::emit_feedback_request_tags_with_auth_env;
use codex_http_client::ClientRouteClass;
use codex_http_client::HttpClientFactory;
use codex_login::AuthEnvTelemetry;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::collect_auth_env_telemetry;
use codex_login::default_client::build_default_reqwest_client_for_route_async;
use codex_model_provider_info::ModelProviderInfo;
use codex_models_manager::manager::ModelsEndpointClient;
use codex_models_manager::manager::ModelsEndpointFuture;
use codex_otel::TelemetryAuthMode;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CoreResult;
use codex_protocol::openai_models::ModelInfo;
use codex_response_debug_context::extract_response_debug_context;
use codex_response_debug_context::telemetry_transport_error_message;
use http::HeaderMap;
use tokio::time::timeout;
use tracing::warn;

use crate::BearerAuthProvider;
use crate::CoreAuthProvider;
use crate::auth::agent_identity_telemetry;
use crate::auth::resolve_provider_auth;

const MODELS_REFRESH_TIMEOUT: Duration = Duration::from_secs(5);
const MODELS_ENDPOINT: &str = "/models";

/// Stable on-disk cache identity for a provider, derived from its base URL so
/// different providers never share a models cache file. Providers without a
/// base URL collapse to their display name.
fn cache_identity_for_provider(provider_info: &ModelProviderInfo) -> String {
    let raw = provider_info
        .base_url
        .as_deref()
        .unwrap_or(provider_info.name.as_str());
    let sanitized: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let collapsed = sanitized
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    collapsed.to_ascii_lowercase()
}

/// Provider-owned OpenAI-compatible `/models` endpoint.
#[derive(Debug)]
pub(crate) struct OpenAiModelsEndpoint {
    provider_info: ModelProviderInfo,
    auth_manager: Option<Arc<AuthManager>>,
    transport_builder: Arc<dyn ModelsTransportBuilder>,
}

impl OpenAiModelsEndpoint {
    pub(crate) fn new(
        provider_info: ModelProviderInfo,
        auth_manager: Option<Arc<AuthManager>>,
    ) -> Self {
        Self {
            provider_info,
            auth_manager,
            transport_builder: Arc::new(RouteAwareModelsTransportBuilder),
        }
    }

    async fn auth(&self) -> Option<CodexAuth> {
        match self.auth_manager.as_ref() {
            Some(auth_manager) => auth_manager.auth().await,
            None => None,
        }
    }

    async fn uses_codex_backend(&self) -> bool {
        self.auth()
            .await
            .as_ref()
            .is_some_and(CodexAuth::uses_codex_backend)
    }

    async fn list_models(
        &self,
        client_version: &str,
        http_client_factory: HttpClientFactory,
    ) -> CoreResult<(Vec<ModelInfo>, Option<String>)> {
        let _timer =
            codex_otel::start_global_timer("codex.remote_models.fetch_update.duration_ms", &[]);
        let auth = self.auth().await;
        let auth_mode = auth.as_ref().map(CodexAuth::auth_mode);
        let api_provider = self.provider_info.to_api_provider(auth_mode)?;
        let api_auth = resolve_provider_auth(
            auth.as_ref(),
            &self.provider_info,
            self.auth_manager.as_deref().map(AuthManager::codex_home),
        )
        .await?;
        let request_url =
            ModelsClient::<ReqwestTransport>::request_url(&api_provider, client_version);
        let auth_telemetry = auth_header_telemetry(api_auth.as_ref());
        let agent_identity_telemetry = if let Some(CodexAuth::AgentIdentity(auth)) = auth.as_ref() {
            Some(agent_identity_telemetry(auth))
        } else {
            None
        };
        let request_telemetry: Arc<dyn RequestTelemetry> = Arc::new(ModelsRequestTelemetry {
            auth_mode: auth_mode.map(|mode| TelemetryAuthMode::from(mode).to_string()),
            auth_header_attached: auth_telemetry.attached,
            auth_header_name: auth_telemetry.name,
            agent_identity_telemetry,
            auth_env: self.auth_env(),
        });
        let (mut models, etag) = timeout(MODELS_REFRESH_TIMEOUT, async {
            let transport = self
                .transport_builder
                .build(http_client_factory, request_url.clone())
                .await?;
            let client = ModelsClient::new(transport, api_provider, api_auth)
                .with_telemetry(Some(request_telemetry));
            client
                .list_models(request_url, HeaderMap::new())
                .await
                .map_err(map_api_error)
        })
        .await
        .map_err(|_| CodexErr::Timeout)??;

        // Apply provider-level config overrides for tool calling support
        Self::apply_provider_config_overrides(&mut models, &self.provider_info);

        Ok((models, etag))
    }

    fn apply_provider_config_overrides(
        models: &mut [ModelInfo],
        provider_info: &ModelProviderInfo,
    ) {
        // Override tool calling support if explicitly configured
        if let Some(supports_tool_calling) = provider_info.supports_tool_calling {
            for model in models.iter_mut() {
                model.supports_parallel_tool_calls = supports_tool_calling;
                if supports_tool_calling {
                    model
                        .experimental_supported_tools
                        .push("functions".to_string());
                    model.visibility = codex_protocol::openai_models::ModelVisibility::List;
                }
            }
        }
        // Override parallel tool calls if explicitly configured
        if let Some(supports_parallel) = provider_info.supports_parallel_tool_calls {
            for model in models.iter_mut() {
                model.supports_parallel_tool_calls = supports_parallel;
            }
        }
    }

    fn auth_env(&self) -> AuthEnvTelemetry {
        let codex_api_key_env_enabled = self
            .auth_manager
            .as_ref()
            .is_some_and(|auth_manager| auth_manager.codex_api_key_env_enabled());
        collect_auth_env_telemetry(&self.provider_info, codex_api_key_env_enabled)
    }
}

impl ModelsEndpointClient for OpenAiModelsEndpoint {
    fn has_command_auth(&self) -> bool {
        self.provider_info.has_command_auth()
    }

    fn has_provider_models_endpoint(&self) -> bool {
        // API-key providers (configured via `env_key`) own a `/models` endpoint
        // listing their models with real capability metadata.
        self.provider_info.env_key.is_some()
    }

    fn models_cache_identity(&self) -> String {
        cache_identity_for_provider(&self.provider_info)
    }

    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(OpenAiModelsEndpoint::uses_codex_backend(self))
    }

    fn list_models<'a>(
        &'a self,
        client_version: &'a str,
        http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<(Vec<ModelInfo>, Option<String>)>> {
        Box::pin(OpenAiModelsEndpoint::list_models(
            self,
            client_version,
            http_client_factory,
        ))
    }
}

/// Google Generative Language API (`v1beta/models`) model endpoint.
///
/// Used for the `google` provider (`WireApi::Google`). Lists models live from
/// `GET {base_url}/v1beta/models`, keeping only models that advertise
/// `generateContent`/`streamGenerateContent`, and maps them to `ModelInfo`.
#[derive(Debug)]
pub(crate) struct GoogleModelsEndpoint {
    provider_info: ModelProviderInfo,
    auth_manager: Option<Arc<AuthManager>>,
}

impl GoogleModelsEndpoint {
    pub(crate) fn new(
        provider_info: ModelProviderInfo,
        auth_manager: Option<Arc<AuthManager>>,
    ) -> Self {
        Self {
            provider_info,
            auth_manager,
        }
    }

    async fn auth(&self) -> Option<CodexAuth> {
        match self.auth_manager.as_ref() {
            Some(auth_manager) => auth_manager.auth().await,
            None => None,
        }
    }

    async fn uses_codex_backend(&self) -> bool {
        self.auth()
            .await
            .as_ref()
            .is_some_and(CodexAuth::uses_codex_backend)
    }

    async fn list_models(
        &self,
        _client_version: &str,
        http_client_factory: HttpClientFactory,
    ) -> CoreResult<(Vec<ModelInfo>, Option<String>)> {
        let _timer =
            codex_otel::start_global_timer("codex.remote_models.fetch_update.duration_ms", &[]);
        let auth = self.auth().await;
        let auth_mode = auth.as_ref().map(CodexAuth::auth_mode);
        let api_provider = self.provider_info.to_api_provider(auth_mode)?;

        enum GoogleAuth {
            ApiKey(String),
            OAuth(String),
        }
        let api_key = self
            .provider_info
            .api_key()
            .ok()
            .flatten()
            .or_else(|| {
                auth.as_ref()
                    .and_then(CodexAuth::api_key)
                    .map(str::to_string)
            })
            .or(self.provider_info.experimental_bearer_token.clone());
        let google_auth = match api_key {
            Some(api_key) => GoogleAuth::ApiKey(api_key),
            None => match self.auth_manager.as_ref() {
                Some(auth_manager) => match auth_manager.google_oauth_tokens().await {
                    Some(tokens) => GoogleAuth::OAuth(tokens.access_token),
                    None => return Ok((Vec::new(), None)),
                },
                None => return Ok((Vec::new(), None)),
            },
        };

        let auth_provider: SharedAuthProvider = match &google_auth {
            GoogleAuth::ApiKey(api_key) => Arc::new(CoreAuthProvider {
                token: Some(api_key.clone()),
                account_id: None,
                is_fedramp_account: false,
                token_header_name: Some("x-goog-api-key"),
                use_bearer_prefix: false,
            }),
            GoogleAuth::OAuth(access_token) => Arc::new(BearerAuthProvider {
                token: Some(access_token.clone()),
                account_id: None,
                is_fedramp_account: false,
                token_header_name: None,
                use_bearer_prefix: true,
            }),
        };

        let request_url = api_provider.url_for_path("v1beta/models");
        let transport = build_default_reqwest_client_for_route_async(
            http_client_factory,
            request_url,
            ClientRouteClass::Api,
        )
        .await?;
        let transport = ReqwestTransport::new(transport);

        let client = ApiGoogleGenerateContentClient::new(transport, api_provider, auth_provider);
        // Generative Language model list is best-effort: the bundled catalog
        // stays authoritative when the live call is unavailable or the
        // credential (e.g. OAuth-only free flow) is not accepted there.
        let mut google_models: Vec<GoogleModel> =
            match client.list_all_models(None, HeaderMap::new()).await {
                Ok(models) => models,
                Err(err) => {
                    warn!("failed to list generative language models: {err}");
                    Vec::new()
                }
            };

        // Code Assist (free "Login with Google") discovery: the v1internal
        // backend has no list-models RPC, so augment with the model ids
        // tracked by user-quota buckets when running on OAuth.
        if matches!(&google_auth, GoogleAuth::OAuth(_))
            && let Some(auth_manager) = &self.auth_manager
            && let Ok(Some(ids)) = auth_manager.code_assist_quota_models().await
        {
            for id in ids {
                google_models.push(GoogleModel {
                    name: format!("models/{id}"),
                    version: None,
                    display_name: Some(id),
                    description: None,
                    input_token_limit: None,
                    output_token_limit: None,
                    supported_generation_methods: Some(vec!["generateContent".to_string()]),
                    full_name: None,
                });
            }
        }

        let models = google_models_to_model_infos(&google_models);
        Ok((models, None))
    }
}

impl ModelsEndpointClient for GoogleModelsEndpoint {
    fn has_command_auth(&self) -> bool {
        self.provider_info.has_command_auth()
    }

    fn has_provider_models_endpoint(&self) -> bool {
        true
    }

    fn models_cache_identity(&self) -> String {
        cache_identity_for_provider(&self.provider_info)
    }

    fn uses_codex_backend(&self) -> ModelsEndpointFuture<'_, bool> {
        Box::pin(GoogleModelsEndpoint::uses_codex_backend(self))
    }

    fn list_models<'a>(
        &'a self,
        client_version: &'a str,
        http_client_factory: HttpClientFactory,
    ) -> ModelsEndpointFuture<'a, CoreResult<(Vec<ModelInfo>, Option<String>)>> {
        Box::pin(GoogleModelsEndpoint::list_models(
            self,
            client_version,
            http_client_factory,
        ))
    }
}

type ModelsTransportFuture<'a> =
    Pin<Box<dyn Future<Output = std::io::Result<ReqwestTransport>> + Send + 'a>>;

/// Builds the concrete transport selected for one models request.
///
/// Implementations must honor the supplied request-time client factory and exact request URL.
trait ModelsTransportBuilder: fmt::Debug + Send + Sync {
    fn build(
        &self,
        http_client_factory: HttpClientFactory,
        request_url: String,
    ) -> ModelsTransportFuture<'_>;
}

#[derive(Debug)]
struct RouteAwareModelsTransportBuilder;

impl ModelsTransportBuilder for RouteAwareModelsTransportBuilder {
    fn build(
        &self,
        http_client_factory: HttpClientFactory,
        request_url: String,
    ) -> ModelsTransportFuture<'_> {
        Box::pin(async move {
            build_default_reqwest_client_for_route_async(
                http_client_factory,
                request_url,
                ClientRouteClass::Api,
            )
            .await
            .map(ReqwestTransport::new)
        })
    }
}

#[derive(Clone)]
struct ModelsRequestTelemetry {
    auth_mode: Option<String>,
    auth_header_attached: bool,
    auth_header_name: Option<&'static str>,
    agent_identity_telemetry: Option<AgentIdentityTelemetry>,
    auth_env: AuthEnvTelemetry,
}

impl RequestTelemetry for ModelsRequestTelemetry {
    fn on_request(
        &self,
        attempt: u64,
        status: Option<http::StatusCode>,
        error: Option<&TransportError>,
        duration: Duration,
    ) {
        let success = status.is_some_and(|code| code.is_success()) && error.is_none();
        let error_message = error.map(telemetry_transport_error_message);
        let response_debug = error
            .map(extract_response_debug_context)
            .unwrap_or_default();
        let status = status.map(|status| status.as_u16());
        tracing::event!(
            target: "codex_otel.log_only",
            tracing::Level::INFO,
            event.name = "codex.api_request",
            duration_ms = %duration.as_millis(),
            http.response.status_code = status,
            success = success,
            error.message = error_message.as_deref(),
            attempt = attempt,
            endpoint = MODELS_ENDPOINT,
            auth.header_attached = self.auth_header_attached,
            auth.header_name = self.auth_header_name,
            auth.env_openai_api_key_present = self.auth_env.openai_api_key_env_present,
            auth.env_codex_api_key_present = self.auth_env.codex_api_key_env_present,
            auth.env_codex_api_key_enabled = self.auth_env.codex_api_key_env_enabled,
            auth.env_provider_key_name = self.auth_env.provider_env_key_name.as_deref(),
            auth.env_provider_key_present = self.auth_env.provider_env_key_present,
            auth.env_refresh_token_url_override_present = self.auth_env.refresh_token_url_override_present,
            auth.request_id = response_debug.request_id.as_deref(),
            auth.cf_ray = response_debug.cf_ray.as_deref(),
            auth.error = response_debug.auth_error.as_deref(),
            auth.error_code = response_debug.auth_error_code.as_deref(),
            auth.mode = self.auth_mode.as_deref(),
            auth.agent_id = self.agent_identity_telemetry.as_ref().map(|metadata| metadata.agent_id.as_str()),
            auth.task_id = self.agent_identity_telemetry.as_ref().map(|metadata| metadata.task_id.as_str()),
        );
        tracing::event!(
            target: "codex_otel.trace_safe",
            tracing::Level::INFO,
            event.name = "codex.api_request",
            duration_ms = %duration.as_millis(),
            http.response.status_code = status,
            success = success,
            error.message = error_message.as_deref(),
            attempt = attempt,
            endpoint = MODELS_ENDPOINT,
            auth.header_attached = self.auth_header_attached,
            auth.header_name = self.auth_header_name,
            auth.env_openai_api_key_present = self.auth_env.openai_api_key_env_present,
            auth.env_codex_api_key_present = self.auth_env.codex_api_key_env_present,
            auth.env_codex_api_key_enabled = self.auth_env.codex_api_key_env_enabled,
            auth.env_provider_key_name = self.auth_env.provider_env_key_name.as_deref(),
            auth.env_provider_key_present = self.auth_env.provider_env_key_present,
            auth.env_refresh_token_url_override_present = self.auth_env.refresh_token_url_override_present,
            auth.request_id = response_debug.request_id.as_deref(),
            auth.cf_ray = response_debug.cf_ray.as_deref(),
            auth.error = response_debug.auth_error.as_deref(),
            auth.error_code = response_debug.auth_error_code.as_deref(),
            auth.mode = self.auth_mode.as_deref(),
            auth.agent_id = self.agent_identity_telemetry.as_ref().map(|metadata| metadata.agent_id.as_str()),
            auth.task_id = self.agent_identity_telemetry.as_ref().map(|metadata| metadata.task_id.as_str()),
        );
        emit_feedback_request_tags_with_auth_env(
            &FeedbackRequestTags {
                endpoint: MODELS_ENDPOINT,
                auth_header_attached: self.auth_header_attached,
                auth_header_name: self.auth_header_name,
                auth_mode: self.auth_mode.as_deref(),
                auth_retry_after_unauthorized: None,
                auth_recovery_mode: None,
                auth_recovery_phase: None,
                auth_connection_reused: None,
                auth_request_id: response_debug.request_id.as_deref(),
                auth_cf_ray: response_debug.cf_ray.as_deref(),
                auth_error: response_debug.auth_error.as_deref(),
                auth_error_code: response_debug.auth_error_code.as_deref(),
                auth_recovery_followup_success: None,
                auth_recovery_followup_status: None,
            },
            &self.auth_env,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::sync::Mutex;

    use super::*;
    use codex_http_client::OutboundProxyPolicy;
    use codex_login::default_client::build_reqwest_client;
    use codex_protocol::config_types::ModelProviderAuthInfo;
    use codex_protocol::openai_models::ModelsResponse;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::matchers::query_param;

    fn provider_with_env_key(env_key: Option<&str>) -> ModelProviderInfo {
        ModelProviderInfo {
            name: "Deploy".to_string(),
            base_url: Some("https://api-cdn.deploy.com/api/v1".to_string()),
            env_key: env_key.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn env_key_providers_refresh_their_own_models_endpoint() {
        let provider = provider_with_env_key(Some("DEPLOY_API_KEY"));
        let endpoint = OpenAiModelsEndpoint::new(provider, None);
        assert!(endpoint.has_provider_models_endpoint());
        assert_eq!(
            endpoint.models_cache_identity(),
            "https-api-cdn.deploy.com-api-v1"
        );
    }

    #[test]
    fn providers_without_env_key_do_not_refresh_provider_models() {
        let provider = provider_with_env_key(None);
        let endpoint = OpenAiModelsEndpoint::new(provider, None);
        assert!(!endpoint.has_provider_models_endpoint());
    }

    #[derive(Debug)]
    struct RecordingTransportBuilder {
        observed_request: Arc<Mutex<Option<(OutboundProxyPolicy, String)>>>,
    }

    impl ModelsTransportBuilder for RecordingTransportBuilder {
        fn build(
            &self,
            http_client_factory: HttpClientFactory,
            request_url: String,
        ) -> ModelsTransportFuture<'_> {
            let observed_request = Arc::clone(&self.observed_request);
            Box::pin(async move {
                *observed_request
                    .lock()
                    .expect("observed request lock should not be poisoned") =
                    Some((http_client_factory.outbound_proxy_policy(), request_url));
                Ok(ReqwestTransport::new(build_reqwest_client()))
            })
        }
    }

    fn provider_info_with_command_auth() -> ModelProviderInfo {
        ModelProviderInfo {
            auth: Some(ModelProviderAuthInfo {
                command: "print-token".to_string(),
                args: Vec::new(),
                timeout_ms: NonZeroU64::new(5_000).expect("timeout should be non-zero"),
                refresh_interval_ms: 300_000,
                cwd: std::env::current_dir()
                    .expect("current dir should be available")
                    .try_into()
                    .expect("current dir should be absolute"),
            }),
            requires_openai_auth: false,
            ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
        }
    }

    #[test]
    fn command_auth_provider_reports_command_auth_without_cached_auth() {
        let endpoint = OpenAiModelsEndpoint::new(
            provider_info_with_command_auth(),
            /*auth_manager*/ None,
        );

        assert!(endpoint.has_command_auth());
    }

    #[test]
    fn provider_without_command_auth_reports_no_command_auth() {
        let endpoint = OpenAiModelsEndpoint::new(
            ModelProviderInfo::create_openai_provider(/*base_url*/ None),
            /*auth_manager*/ None,
        );

        assert!(!endpoint.has_command_auth());
    }

    #[tokio::test]
    async fn model_request_uses_request_time_proxy_policy_and_exact_url() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(query_param("client_version", "0.0.0"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(ModelsResponse { models: Vec::new() }),
            )
            .expect(1)
            .mount(&server)
            .await;

        let observed_request = Arc::new(Mutex::new(None));
        let endpoint = OpenAiModelsEndpoint {
            provider_info: ModelProviderInfo::create_openai_provider(Some(server.uri())),
            auth_manager: None,
            transport_builder: Arc::new(RecordingTransportBuilder {
                observed_request: Arc::clone(&observed_request),
            }),
        };

        endpoint
            .list_models(
                "0.0.0",
                HttpClientFactory::new(OutboundProxyPolicy::RespectSystemProxy),
            )
            .await
            .expect("models request should succeed");

        assert_eq!(
            *observed_request
                .lock()
                .expect("observed request lock should not be poisoned"),
            Some((
                OutboundProxyPolicy::RespectSystemProxy,
                format!("{}/models?client_version=0.0.0", server.uri()),
            ))
        );
    }

    #[tokio::test]
    async fn google_endpoint_lists_generate_content_models() {
        use codex_model_provider_info::WireApi;

        let server = MockServer::start().await;
        unsafe {
            std::env::set_var("GOOGLE_API_KEY", "test-key");
        }
        Mock::given(method("GET"))
            .and(path("/v1beta/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [
                    {
                        "name": "models/gemini-2.5-flash",
                        "displayName": "Gemini 2.5 Flash",
                        "inputTokenLimit": 1_048_576,
                        "outputTokenLimit": 65_536,
                        "supportedGenerationMethods": ["generateContent", "streamGenerateContent"]
                    },
                    {
                        "name": "models/text-embedding-004",
                        "displayName": "Text Embedding 004",
                        "supportedGenerationMethods": ["embedContent"]
                    },
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let provider_info = ModelProviderInfo {
            name: "Google".to_string(),
            base_url: Some(server.uri()),
            env_key: Some("GOOGLE_API_KEY".to_string()),
            wire_api: WireApi::Google,
            ..Default::default()
        };
        let endpoint = GoogleModelsEndpoint::new(provider_info, None);
        let (models, _etag) = endpoint
            .list_models(
                "0.0.0",
                HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
            )
            .await
            .expect("google models request should succeed");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].slug, "gemini-2.5-flash");
        assert_eq!(models[0].display_name, "Gemini 2.5 Flash");
        assert_eq!(models[0].context_window, Some(1_048_576));
        assert!(models[0].supported_in_api);
        // SAFETY(test): restore env for other tests in this binary.
        unsafe { std::env::remove_var("GOOGLE_API_KEY") };
    }
}

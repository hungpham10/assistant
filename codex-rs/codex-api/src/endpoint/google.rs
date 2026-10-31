use crate::auth::SharedAuthProvider;
use crate::common::ResponseStream;
use crate::error::ApiError;
use crate::google::GoogleGenerateContentRequest;
use crate::provider::Provider;
use crate::sse::spawn_google_response_stream;
use crate::telemetry::SseTelemetry;
use codex_client::HttpTransport;
use codex_client::RequestCompression;
use codex_client::RequestTelemetry;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use serde_json::Value;
use std::sync::Arc;
use tracing::instrument;

pub struct GoogleGenerateContentClient<T: HttpTransport> {
    session: crate::endpoint::session::EndpointSession<T>,
    sse_telemetry: Option<Arc<dyn SseTelemetry>>,
}

impl<T: HttpTransport> GoogleGenerateContentClient<T> {
    pub fn new(transport: T, provider: Provider, auth: SharedAuthProvider) -> Self {
        Self {
            session: crate::endpoint::session::EndpointSession::new(transport, provider, auth),
            sse_telemetry: None,
        }
    }

    pub fn with_telemetry(
        self,
        request: Option<Arc<dyn RequestTelemetry>>,
        sse: Option<Arc<dyn SseTelemetry>>,
    ) -> Self {
        Self {
            session: self.session.with_request_telemetry(request),
            sse_telemetry: sse,
        }
    }

    #[instrument(
        name = "google.generate_content",
        level = "info",
        skip_all,
        fields(
            transport = "google_http",
            http.method = "POST"
        )
    )]
    pub async fn stream_request(
        &self,
        model: &str,
        request: GoogleGenerateContentRequest,
        extra_headers: HeaderMap,
    ) -> Result<ResponseStream, ApiError> {
        self.stream(model, request, extra_headers).await
    }

    async fn stream(
        &self,
        model: &str,
        request: GoogleGenerateContentRequest,
        mut extra_headers: HeaderMap,
    ) -> Result<ResponseStream, ApiError> {
        let body = serde_json::to_value(&request)
            .map_err(|err| ApiError::Stream(format!("failed to encode google request: {err}")))?;
        let path = format!("v1beta/models/{model}:streamGenerateContent?alt=sse");
        extra_headers.insert(
            http::header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );
        let stream_response = self
            .session
            .stream_with(Method::POST, &path, extra_headers, Some(body), |req| {
                req.headers.remove("originator");
                req.compression = RequestCompression::None;
            })
            .await?;

        Ok(spawn_google_response_stream(
            stream_response,
            self.session.provider().stream_idle_timeout,
            self.sse_telemetry.clone(),
        ))
    }
}

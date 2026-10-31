use crate::common::ResponseEvent;
use crate::common::ResponseStream;
use crate::error::ApiError;
use crate::google::GoogleGenerateContentResponse;
use crate::telemetry::SseTelemetry;
use codex_client::ByteStream;
use codex_client::StreamResponse;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::debug;
use tracing::trace;

pub fn spawn_google_response_stream(
    stream_response: StreamResponse,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
) -> ResponseStream {
    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent, ApiError>>(1600);
    tokio::spawn(process_sse(
        stream_response.bytes,
        tx_event,
        idle_timeout,
        telemetry,
    ));
    ResponseStream {
        rx_event,
        upstream_request_id: None,
    }
}

pub async fn process_sse(
    stream: ByteStream,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    idle_timeout: Duration,
    _telemetry: Option<Arc<dyn SseTelemetry>>,
) {
    let mut stream = stream.eventsource();
    loop {
        let event = match timeout(idle_timeout, stream.next()).await {
            Ok(Some(Ok(event))) => event,
            Ok(Some(Err(e))) => {
                let _ = tx_event.send(Err(ApiError::Stream(e.to_string()))).await;
                return;
            }
            Ok(None) => return,
            Err(_) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(
                        "idle timeout waiting for response".to_string(),
                    )))
                    .await;
                return;
            }
        };

        if event.data.is_empty() {
            continue;
        }

        match serde_json::from_str::<GoogleGenerateContentResponse>(&event.data) {
            Ok(resp) => {
                if let Some(candidates) = resp.candidates {
                    for candidate in candidates {
                        if let Some(content) = candidate.content {
                            for part in content.parts {
                                match part {
                                    crate::google::GooglePart::Text { text } => {
                                        if !text.is_empty() {
                                            let item = ResponseItem::Message {
                                                id: None,
                                                role: "assistant".to_string(),
                                                content: vec![ContentItem::Thought {
                                                    text: text.clone(),
                                                }]
                                                .into_iter()
                                                .collect(),
                                                phase: None,
                                                internal_chat_message_metadata_passthrough: None,
                                            };
                                            // Try to emit as thought/message - keep it simple
                                            let _ = tx_event
                                                .send(Ok(ResponseEvent::OutputItemDone(item)))
                                                .await;
                                        }
                                    }
                                    crate::google::GooglePart::FunctionCall { function_call } => {
                                        let _ = tx_event
                                            .send(Ok(ResponseEvent::OutputItemDone(
                                                ResponseItem::FunctionCall {
                                                    id: None,
                                                    name: function_call.name,
                                                    arguments: function_call
                                                        .args
                                                        .unwrap_or(serde_json::json!({}))
                                                        .to_string(),
                                                    call_id: String::new(),
                                                },
                                            )))
                                            .await;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                if let Some(usage) = resp.usage_metadata {
                    if let Some(usage_obj) = usage.as_object() {
                        let prompt_tokens = usage_obj
                            .get("promptTokenCount")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let completion_tokens = usage_obj
                            .get("candidatesTokenCount")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let _ = tx_event
                            .send(Ok(ResponseEvent::TokenCount(TokenUsage {
                                input_tokens: prompt_tokens,
                                cached_input_tokens: 0,
                                output_tokens: completion_tokens,
                                reasoning_output_tokens: 0,
                                total_tokens: prompt_tokens + completion_tokens,
                            })))
                            .await;
                    }
                }
            }
            Err(e) => {
                debug!("failed to parse google event: {e}, data: {}", event.data);
            }
        }
    }
}

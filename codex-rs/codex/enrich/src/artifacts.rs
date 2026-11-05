//! Artifact convention for verb outputs.
//!
//! A verb that produces large or binary data returns it under the reserved
//! `"artifact"` key instead of inline JSON:
//!
//! ```json
//! { "artifact": { "path": "/tmp/clip.wav", "kind": "audio/wav", "meta": {} } }
//! ```
//!
//! or with inline bytes: `{ "artifact": { "bytes_b64": "...", "kind": "..." } }`.
//!
//! Before a verb result is recorded into the chain (and eventually shown to
//! the model), [`materialize_artifacts`] pushes the payload into an
//! [`ArtifactSink`] and replaces the entry with the compact
//! `{ artifact_id, path, kind, meta }` reference.

use std::future::Future;
use std::pin::Pin;

use base64::Engine;
use codex_artifacts::ArtifactRef;
use serde_json::Value as JsonValue;

/// Reserved key inside verb outputs that carries artifact payloads.
pub const ARTIFACT_KEY: &str = "artifact";

/// Where materialized artifacts are persisted.
pub trait ArtifactSink: Send + Sync {
    fn store<'a>(
        &'a self,
        bytes: Vec<u8>,
        kind: &'a str,
        meta: JsonValue,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ArtifactRef>> + Send + 'a>>;
}

/// Scans `output` for artifact payloads and rewrites them into compact
/// references. Non-object outputs and objects without the reserved key are
/// left untouched.
pub async fn materialize_artifacts(
    output: &mut JsonValue,
    sink: Option<&dyn ArtifactSink>,
) -> anyhow::Result<()> {
    let Some(sink) = sink else {
        return Ok(());
    };
    let Some(obj) = output.as_object_mut() else {
        return Ok(());
    };
    let Some(payload) = obj.remove(ARTIFACT_KEY) else {
        return Ok(());
    };

    let artifact_ref = materialize_one(payload, sink).await?;
    obj.insert(
        "artifact_id".to_string(),
        JsonValue::String(artifact_ref.artifact_id),
    );
    obj.insert(
        "path".to_string(),
        JsonValue::String(artifact_ref.path.display().to_string()),
    );
    obj.insert("kind".to_string(), JsonValue::String(artifact_ref.kind));
    obj.insert("meta".to_string(), artifact_ref.meta);
    Ok(())
}

async fn materialize_one(
    payload: JsonValue,
    sink: &dyn ArtifactSink,
) -> anyhow::Result<ArtifactRef> {
    let base64_engine = base64::engine::general_purpose::STANDARD;
    let mut fields = payload
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("`{ARTIFACT_KEY}` must be an object"))?
        .clone();

    if let Some(bytes_b64) = fields.remove("bytes_b64") {
        let encoded = bytes_b64
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("`bytes_b64` must be a string"))?;
        let bytes = base64_engine.decode(encoded)?;
        let kind = take_kind(&mut fields)?;
        let meta = fields
            .into_iter()
            .collect::<serde_json::Map<String, JsonValue>>();
        sink.store(bytes, &kind, JsonValue::Object(meta)).await
    } else if let Some(path) = fields.remove("path") {
        let path = path
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("`path` must be a string"))?
            .to_string();
        let bytes = tokio::fs::read(&path).await?;
        let kind = take_kind(&mut fields)?;
        let meta = fields
            .into_iter()
            .collect::<serde_json::Map<String, JsonValue>>();
        sink.store(bytes, &kind, JsonValue::Object(meta)).await
    } else {
        Err(anyhow::anyhow!(
            "`{ARTIFACT_KEY}` requires either `bytes_b64` or `path`"
        ))
    }
}

fn take_kind(fields: &mut serde_json::Map<String, JsonValue>) -> anyhow::Result<String> {
    fields
        .remove("kind")
        .and_then(|v| v.as_str().map(str::to_string))
        .ok_or_else(|| anyhow::anyhow!("`{ARTIFACT_KEY}` requires a `kind` string"))
}

#[cfg(test)]
#[path = "artifacts_tests.rs"]
mod tests;

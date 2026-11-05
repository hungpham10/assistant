use super::*;
use std::sync::Mutex;

struct MemorySink {
    stored: Mutex<Vec<(Vec<u8>, String)>>,
    next_id: std::sync::atomic::AtomicUsize,
}

impl MemorySink {
    fn new() -> Self {
        Self {
            stored: Mutex::new(Vec::new()),
            next_id: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

fn leak_sink() -> &'static MemorySink {
    // Test process is short-lived; leaking keeps 'static references simple.
    Box::leak(Box::new(MemorySink::new()))
}

impl ArtifactSink for MemorySink {
    fn store<'a>(
        &'a self,
        bytes: Vec<u8>,
        kind: &'a str,
        meta: JsonValue,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ArtifactRef>> + Send + 'a>> {
        Box::pin(async move {
            self.stored.lock().unwrap().push((bytes, kind.to_string()));
            let id = self
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ArtifactRef {
                artifact_id: format!("a-{id}"),
                path: format!("/tmp/artifacts/a-{id}").into(),
                kind: kind.to_string(),
                meta,
            })
        })
    }
}

#[tokio::test]
async fn inline_bytes_become_artifact_reference() {
    let sink = leak_sink();
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(b"WAVEBYTES");
    let mut output = serde_json::json!({
        "artifact": { "bytes_b64": encoded, "kind": "audio/wav", "duration_s": 3 },
        "note": "kept"
    });

    materialize_artifacts(&mut output, Some(sink))
        .await
        .unwrap();

    assert_eq!(output["artifact_id"], serde_json::json!("a-0"));
    assert_eq!(output["kind"], "audio/wav");
    assert_eq!(output["meta"], serde_json::json!({ "duration_s": 3 }));
    assert_eq!(output["note"], "kept");
    assert_eq!(sink.stored.lock().unwrap()[0].0, b"WAVEBYTES".to_vec());
}

#[tokio::test]
async fn path_payload_is_read_and_stored() {
    let sink = leak_sink();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("frame.png");
    std::fs::write(&src, b"PNGDATA").unwrap();

    let mut output = serde_json::json!({
        "artifact": { "path": src.display().to_string(), "kind": "image/png" }
    });

    materialize_artifacts(&mut output, Some(sink))
        .await
        .unwrap();

    assert_eq!(output["artifact_id"], serde_json::json!("a-0"));
    assert_eq!(sink.stored.lock().unwrap()[0].0, b"PNGDATA".to_vec());
}

#[tokio::test]
async fn output_without_artifact_key_is_untouched() {
    let sink = leak_sink();
    let mut output = serde_json::json!({ "result": "plain" });
    materialize_artifacts(&mut output, Some(sink))
        .await
        .unwrap();
    assert_eq!(output, serde_json::json!({ "result": "plain" }));
}

#[tokio::test]
async fn missing_kind_is_an_error() {
    let sink = leak_sink();
    let mut output = serde_json::json!({ "artifact": { "bytes_b64": "aGk=" } });
    assert!(
        materialize_artifacts(&mut output, Some(sink))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn no_sink_leaves_output_untouched() {
    let mut output = serde_json::json!({
        "artifact": { "bytes_b64": "aGk=", "kind": "text/plain" }
    });
    materialize_artifacts(&mut output, None).await.unwrap();
    assert!(output["artifact"].is_object());
}

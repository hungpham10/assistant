use super::*;
use pretty_assertions::assert_eq;

fn codex_home() -> AbsolutePathBuf {
    let dir = tempfile::tempdir().unwrap();
    // Leak so the temp dir outlives the store in these small tests.
    let path = dir.keep();
    AbsolutePathBuf::from_absolute_path(path).unwrap()
}

#[tokio::test]
async fn put_then_read_round_trips_bytes() {
    let store = ArtifactStore::new(&codex_home(), "thread-1").unwrap();
    let artifact_ref = store
        .put(
            b"hello artifact".to_vec(),
            "text/plain",
            serde_json::json!({}),
        )
        .await
        .unwrap();

    assert!(artifact_ref.artifact_id.starts_with("a-"));
    assert_eq!(artifact_ref.kind, "text/plain");
    let bytes = store.read(&artifact_ref).await.unwrap();
    assert_eq!(bytes, b"hello artifact".to_vec());
}

#[tokio::test]
async fn put_from_path_moves_contents() {
    let store = ArtifactStore::new(&codex_home(), "thread-2").unwrap();
    let src_dir = tempfile::tempdir().unwrap();
    let src = src_dir.path().join("clip.wav");
    std::fs::write(&src, b"RIFF....").unwrap();

    let artifact_ref = store
        .put_from_path(&src, "audio/wav", serde_json::json!({ "duration_s": 1 }))
        .await
        .unwrap();

    assert_eq!(artifact_ref.kind, "audio/wav");
    assert_eq!(artifact_ref.meta, serde_json::json!({ "duration_s": 1 }));
    let bytes = store.read(&artifact_ref).await.unwrap();
    assert_eq!(bytes, b"RIFF....".to_vec());
}

#[tokio::test]
async fn cleanup_expired_respects_ttl() {
    let store = ArtifactStore::new(&codex_home(), "thread-3").unwrap();
    let artifact_ref = store
        .put(b"fresh".to_vec(), "text/plain", serde_json::json!({}))
        .await
        .unwrap();

    // A generous TTL keeps the just-written artifact.
    let removed = store
        .cleanup_expired(Duration::from_secs(3600))
        .await
        .unwrap();
    assert_eq!(removed, 0);
    assert!(artifact_ref.path.exists());

    // A zero TTL expires everything.
    let removed = store.cleanup_expired(Duration::ZERO).await.unwrap();
    assert_eq!(removed, 1);
    assert!(!artifact_ref.path.exists());
}

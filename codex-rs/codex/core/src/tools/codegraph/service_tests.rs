//! Lifecycle tests for [`super::CodegraphService`].
//!
//! These touch the process-global session, so they run serially.

use super::CodegraphService;
use pretty_assertions::assert_eq;
use serde_json::json;
use serial_test::serial;

fn temp_project_with_source() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("lib.rs"),
        "pub fn hello_world() {}\n\npub fn caller_of_hello() {\n    hello_world();\n}\n",
    )
    .expect("write source");
    dir
}

#[tokio::test]
#[serial]
async fn search_before_begin_returns_begin_guidance() {
    let _ = CodegraphService::end().await;
    let out = CodegraphService::search("search_symbol", &json!({"query": "hello_world"})).await;
    assert!(
        out.contains("no codegraph session bound") && out.contains("begin(path=...)"),
        "unexpected output: {out}"
    );
}

#[tokio::test]
#[serial]
async fn begin_is_bind_only_and_suggests_index_for_empty_index() {
    let dir = temp_project_with_source();
    let path = dir.path().to_str().expect("utf8 path");
    let out = CodegraphService::begin(path).await;
    assert!(out.starts_with("begin ok"), "unexpected output: {out}");
    assert!(out.contains("/index"), "expected /index hint: {out}");
    // `.codegraph/` was created but nothing was indexed: stats are empty.
    assert!(dir.path().join(".codegraph").exists());
    let stats = CodegraphService::search("stats", &json!({})).await;
    let stats: serde_json::Value = serde_json::from_str(&stats).expect("stats json");
    assert_eq!(stats["symbols"], 0);
    let _ = CodegraphService::end().await;
}

#[tokio::test]
#[serial]
async fn host_index_then_search_returns_symbols() {
    let dir = temp_project_with_source();
    let path = dir.path().to_str().expect("utf8 path");
    let summary = CodegraphService::host_index(path, "all")
        .await
        .expect("host index");
    assert!(summary.symbols > 0, "expected symbols, got {summary:?}");

    let _ = CodegraphService::begin(path).await;
    let out = CodegraphService::search("search_symbol", &json!({"query": "hello_world"})).await;
    let v: serde_json::Value = serde_json::from_str(&out).expect("search json");
    let names: Vec<&str> = v["symbols"]
        .as_array()
        .expect("symbols array")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert!(
        names.contains(&"hello_world"),
        "expected hello_world in {names:?} (raw: {out})"
    );

    let status = CodegraphService::status().await;
    assert!(status.contains("bound="), "unexpected status: {status}");
    assert!(
        !status.contains("bound=<none>"),
        "unexpected status: {status}"
    );

    let _ = CodegraphService::end().await;

    // Strict lifecycle: search after end fails until begin again.
    let out = CodegraphService::search("stats", &json!({})).await;
    assert!(
        out.contains("no codegraph session bound"),
        "unexpected output: {out}"
    );
}

#[tokio::test]
#[serial]
async fn agent_cannot_index_via_search_action() {
    let dir = temp_project_with_source();
    let path = dir.path().to_str().expect("utf8 path");
    let _ = CodegraphService::begin(path).await;
    let out = CodegraphService::search("index", &json!({})).await;
    assert!(
        out.contains("unknown search action"),
        "unexpected output: {out}"
    );
    let _ = CodegraphService::end().await;
}

#[tokio::test]
#[serial]
async fn begin_with_bad_path_fails_without_binding() {
    let _ = CodegraphService::end().await;
    let out = CodegraphService::begin("/definitely/not/a/real/path").await;
    assert!(out.starts_with("begin failed"), "unexpected output: {out}");
    let status = CodegraphService::status().await;
    assert!(status.contains("not bound"), "unexpected status: {status}");
}

#[tokio::test]
#[serial]
async fn docs_only_index_ingests_configured_documents() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("config.json"),
        r#"{"name": "app", "replicas": 2}"#,
    )
    .expect("write doc");
    std::fs::create_dir_all(dir.path().join(".codegraph")).expect("codegraph dir");
    std::fs::write(
        dir.path().join(".codegraph/config.toml"),
        "[docgraph]\npaths = [\"config.json\"]\n",
    )
    .expect("write config");
    let path = dir.path().to_str().expect("utf8 path");

    let summary = CodegraphService::host_index(path, "docs")
        .await
        .expect("docs index");
    assert!(
        summary.docs_configured,
        "expected configured docs: {summary:?}"
    );
    assert_eq!(summary.docs, 1);
    assert!(summary.doc_nodes > 0, "expected doc nodes: {summary:?}");

    let _ = CodegraphService::begin(path).await;
    let out = CodegraphService::search("doc_list", &json!({})).await;
    let v: serde_json::Value = serde_json::from_str(&out).expect("doc_list json");
    assert_eq!(v["total"], 1);
    let doc_path = v["docs"][0]["path"].as_str().expect("path");
    assert!(
        doc_path.ends_with("config.json"),
        "unexpected doc path: {doc_path}"
    );

    let out = CodegraphService::search("doc_search", &json!({"query": "replicas"})).await;
    let v: serde_json::Value = serde_json::from_str(&out).expect("doc_search json");
    assert!(
        v["total"].as_u64().expect("total") >= 1,
        "unexpected: {out}"
    );

    let out = CodegraphService::search("doc_stats", &json!({})).await;
    let v: serde_json::Value = serde_json::from_str(&out).expect("doc_stats json");
    assert_eq!(v["docs"], 1);
    let _ = CodegraphService::end().await;
}

#[tokio::test]
#[serial]
async fn source_only_index_skips_documents() {
    let dir = temp_project_with_source();
    std::fs::write(dir.path().join("config.json"), r#"{"name": "app"}"#).expect("write doc");
    std::fs::create_dir_all(dir.path().join(".codegraph")).expect("codegraph dir");
    std::fs::write(
        dir.path().join(".codegraph/config.toml"),
        "[docgraph]\npaths = [\"config.json\"]\n",
    )
    .expect("write config");
    let path = dir.path().to_str().expect("utf8 path");

    let summary = CodegraphService::host_index(path, "source")
        .await
        .expect("source index");
    assert!(summary.symbols > 0, "expected symbols: {summary:?}");
    assert!(
        !summary.docs_configured,
        "docs must be skipped: {summary:?}"
    );
    assert_eq!(summary.docs, 0);
}

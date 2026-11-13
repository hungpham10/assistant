use super::*;
use pretty_assertions::assert_eq;

use codex_protocol::protocol::SessionSource;

fn record(thread_id: &str, strategy_name: &str, turn_id: &str) -> StrategyStateRecord {
    StrategyStateRecord {
        thread_id: thread_id.to_string(),
        strategy_name: strategy_name.to_string(),
        model_json: format!(r#"{{"strategy":"{strategy_name}"}}"#),
        captured_at: 1_700_000_000,
        turn_id: turn_id.to_string(),
        updated_at: 1_700_000_100,
    }
}

async fn runtime() -> Arc<StateRuntime> {
    let codex_home = super::test_support::unique_temp_dir();
    StateRuntime::init(codex_home, "test-provider".to_string())
        .await
        .expect("state runtime")
}

/// Creates the runtime plus one parent thread (the FK requires it) and
/// returns the thread id string.
async fn runtime_with_thread() -> (Arc<StateRuntime>, String) {
    let runtime = runtime().await;
    let thread_id = ThreadId::new();
    let metadata = crate::ThreadMetadataBuilder::new(
        thread_id,
        "/tmp/rollout.jsonl".into(),
        chrono::Utc::now(),
        SessionSource::Exec,
    )
    .build("test-provider");
    runtime
        .upsert_thread(&metadata)
        .await
        .expect("insert thread");
    (runtime, thread_id.to_string())
}

#[tokio::test]
async fn upsert_and_get_strategy_state() {
    let (runtime, thread_id) = runtime_with_thread().await;
    let record = record(&thread_id, "explore-code", "turn-1");
    runtime
        .upsert_strategy_state(&record)
        .await
        .expect("upsert");

    let stored = runtime
        .get_strategy_state(&thread_id)
        .await
        .expect("get")
        .expect("row exists");
    assert_eq!(stored, record);

    assert!(
        runtime
            .get_strategy_state("missing")
            .await
            .expect("get")
            .is_none()
    );
}

#[tokio::test]
async fn upsert_replaces_the_previous_snapshot() {
    let (runtime, thread_id) = runtime_with_thread().await;
    runtime
        .upsert_strategy_state(&record(&thread_id, "explore-code", "turn-1"))
        .await
        .expect("upsert");
    let updated = StrategyStateRecord {
        strategy_name: "write-code".to_string(),
        captured_at: 1_700_000_200,
        turn_id: "turn-2".to_string(),
        updated_at: 1_700_000_300,
        ..record(&thread_id, "explore-code", "turn-1")
    };
    runtime
        .upsert_strategy_state(&updated)
        .await
        .expect("upsert");

    let stored = runtime
        .get_strategy_state(&thread_id)
        .await
        .expect("get")
        .expect("row exists");
    assert_eq!(stored, updated);
}

#[tokio::test]
async fn delete_strategy_state_removes_the_row() {
    let (runtime, thread_id) = runtime_with_thread().await;
    runtime
        .upsert_strategy_state(&record(&thread_id, "explore-code", "turn-1"))
        .await
        .expect("upsert");
    runtime
        .delete_strategy_state(&thread_id)
        .await
        .expect("delete");
    assert!(
        runtime
            .get_strategy_state(&thread_id)
            .await
            .expect("get")
            .is_none()
    );
}

#[tokio::test]
async fn strategy_state_cascades_on_thread_delete() {
    let (runtime, thread_id) = runtime_with_thread().await;
    runtime
        .upsert_strategy_state(&record(&thread_id, "explore-code", "turn-1"))
        .await
        .expect("upsert");

    let thread = ThreadId::from_string(&thread_id).expect("valid thread id");
    runtime
        .delete_threads_strict(&[thread])
        .await
        .expect("delete thread");
    assert!(
        runtime
            .get_strategy_state(&thread_id)
            .await
            .expect("get")
            .is_none(),
        "strategy_state row must cascade with the thread"
    );
}

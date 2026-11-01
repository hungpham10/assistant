use super::*;
use pretty_assertions::assert_eq;

fn record(
    thread_id: &str,
    model: &str,
    input: i64,
    output: i64,
    created_at: i64,
) -> TurnUsageRecord {
    TurnUsageRecord {
        thread_id: thread_id.to_string(),
        turn_id: format!("turn-{thread_id}-{created_at}"),
        model: model.to_string(),
        input_tokens: input,
        cached_input_tokens: input / 2,
        output_tokens: output,
        reasoning_output_tokens: 0,
        created_at,
    }
}

async fn runtime() -> Arc<StateRuntime> {
    let codex_home = super::test_support::unique_temp_dir();
    StateRuntime::init(codex_home, "test-provider".to_string())
        .await
        .expect("state runtime")
}

#[tokio::test]
async fn records_and_summarizes_turn_usage() {
    let runtime = runtime().await;
    runtime
        .record_turn_usage(&record(
            "t1", "gemini-x", /*input*/ 100, /*output*/ 50, 1_000,
        ))
        .await
        .expect("insert");
    runtime
        .record_turn_usage(&record(
            "t2", "gemini-x", /*input*/ 200, /*output*/ 70, 2_000,
        ))
        .await
        .expect("insert");
    runtime
        .record_turn_usage(&record(
            "t3", "other-y", /*input*/ 10, /*output*/ 5, 3_000,
        ))
        .await
        .expect("insert");

    let total = runtime
        .summarize_turn_usage(/*since_epoch_seconds*/ None)
        .await
        .expect("summary");
    assert_eq!(
        total,
        TurnUsageSummary {
            request_count: 3,
            input_tokens: 310,
            cached_input_tokens: 155,
            output_tokens: 125,
            reasoning_output_tokens: 0,
        }
    );

    let since = runtime
        .summarize_turn_usage(Some(2_000))
        .await
        .expect("summary");
    assert_eq!(since.request_count, 2);
    assert_eq!(since.input_tokens, 210);
}

#[tokio::test]
async fn summarize_by_model_orders_by_total_tokens() {
    let runtime = runtime().await;
    runtime
        .record_turn_usage(&record(
            "t1", "small-m", /*input*/ 10, /*output*/ 5, 1_000,
        ))
        .await
        .expect("insert");
    runtime
        .record_turn_usage(&record(
            "t2", "big-m", /*input*/ 500, /*output*/ 300, 1_000,
        ))
        .await
        .expect("insert");

    let grouped = runtime
        .summarize_turn_usage_by_model(/*since_epoch_seconds*/ None)
        .await
        .expect("summary");
    assert_eq!(grouped.len(), 2);
    assert_eq!(grouped[0].model, "big-m");
    assert_eq!(grouped[0].summary.input_tokens, 500);
    assert_eq!(grouped[1].model, "small-m");
}

#[tokio::test]
async fn prunes_rows_older_than_cutoff() {
    let runtime = runtime().await;
    runtime
        .record_turn_usage(&record(
            "t1", "m", /*input*/ 10, /*output*/ 5, 1_000,
        ))
        .await
        .expect("insert");
    runtime
        .record_turn_usage(&record(
            "t2", "m", /*input*/ 20, /*output*/ 6, 9_000,
        ))
        .await
        .expect("insert");

    let pruned = runtime.prune_turn_usage(5_000).await.expect("prune");
    assert_eq!(pruned, 1);

    let total = runtime
        .summarize_turn_usage(/*since_epoch_seconds*/ None)
        .await
        .expect("summary");
    assert_eq!(total.request_count, 1);
    assert_eq!(total.input_tokens, 20);
}

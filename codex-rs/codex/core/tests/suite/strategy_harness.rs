use anyhow::Result;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::StrategyFeedbackChoice;
use codex_protocol::protocol::StrategyFeedbackRequest;
use codex_protocol::protocol::StrategyStateSnapshot;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use core_test_support::wait_for_event_match;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// One-node strategy with a deterministic error-condition edge so the walk
/// moves `a → b` after the mocked (failing) tool call.
const STRATEGY_TOML: &str = r#"
[strategy]
name = "test-strategy"
[origin]
a = 1.0
[node.a]
guidance = "start here"
budget = 4
backtrack_to = "a"
[[node.a.edges]]
to = "b"
when = "tool_result:lookup_thing:error"
likelihood = 1.0
[node.b]
guidance = "then here"
budget = 4
backtrack_to = "a"
"#;

const STRATEGY_NAME: &str = "test-strategy";

fn write_strategy_file(cwd: &Path) -> Result<()> {
    let dir = cwd.join(".assistant/strategies");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(format!("{STRATEGY_NAME}.toml")), STRATEGY_TOML)?;
    Ok(())
}

fn strategy_items(rollout_path: &Path) -> Result<Vec<StrategyStateSnapshot>> {
    let text = fs::read_to_string(rollout_path)?;
    let mut snapshots = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if let Ok(item) = serde_json::from_str::<RolloutItem>(line)
            && let RolloutItem::StrategyState(snapshot) = item
        {
            snapshots.push(snapshot);
        }
    }
    Ok(snapshots)
}

fn strategy_context_entry(
    name: &str,
) -> BTreeMap<String, codex_protocol::protocol::AdditionalContextEntry> {
    BTreeMap::from([(
        "strategy".to_string(),
        codex_protocol::protocol::AdditionalContextEntry {
            value: format!("activate:{name}"),
            kind: codex_protocol::protocol::AdditionalContextKind::Application,
        },
    )])
}

async fn submit_lookup_turn(
    test: &core_test_support::test_codex::TestCodex,
    server: &wiremock::MockServer,
) -> Result<StrategyFeedbackRequest> {
    // The model calls an unknown tool (error result), then finishes.
    mount_sse_once_match(
        server,
        |request: &wiremock::Request| {
            serde_json::from_slice::<serde_json::Value>(&request.body)
                .is_ok_and(|body| body.to_string().contains("run the lookup"))
        },
        sse(vec![
            ev_response_created("resp-1"),
            ev_function_call("call-1", "lookup_thing", "{}"),
            ev_completed("resp-1"),
        ]),
    )
    .await;
    let _post_tool_mock = mount_sse_once_match(
        server,
        |request: &wiremock::Request| {
            serde_json::from_slice::<serde_json::Value>(&request.body).is_ok_and(|body| {
                body.to_string().contains("call-1") && !body.to_string().contains("run the lookup")
            })
        },
        sse(vec![ev_response_created("resp-2"), ev_completed("resp-2")]),
    )
    .await;

    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "run the lookup".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: strategy_context_entry(STRATEGY_NAME),
            thread_settings: Default::default(),
        })
        .await?;
    let request = wait_for_event_match(&test.codex, |event| match event {
        EventMsg::StrategyFeedbackRequest(request) => Some(request.clone()),
        _ => None,
    })
    .await;
    assert_eq!(request.strategy_name, STRATEGY_NAME);
    assert_eq!(request.node.as_deref(), Some("b"), "walk advanced a → b");
    Ok(request)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn strategy_turn_persists_snapshot_and_feedback_updates_model() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let test = test_codex().build(&server).await?;
    write_strategy_file(&test.cwd.path())?;

    let request = submit_lookup_turn(&test, &server).await?;

    // The step snapshot was persisted before the prompt; feedback re-persists
    // the model with the boosted tool weight.
    test.codex
        .submit(Op::StrategyFeedback {
            id: request.id,
            choice: StrategyFeedbackChoice::Helpful,
        })
        .await?;
    test.codex.flush_rollout().await?;

    let rollout_path = test.codex.rollout_path().expect("rollout path");
    let snapshots = strategy_items(&rollout_path)?;
    assert!(
        snapshots.len() >= 2,
        "expected the step snapshot plus the post-feedback snapshot, got {}",
        snapshots.len()
    );
    let model = &snapshots.last().expect("at least one snapshot").model;
    let weight = model["b"]["a"]["lookup_thing"]
        .as_f64()
        .expect("feedback entered the called tool into the B row");
    assert!(
        weight > 1.0,
        "helpful feedback should boost the tool above the neutral weight, got {weight}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn strategy_state_is_restored_on_resume() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let test = test_codex().build(&server).await?;
    write_strategy_file(&test.cwd.path())?;
    let home = Arc::clone(&test.home);

    // Activate the strategy and advance the walk with a failed tool call.
    let request = submit_lookup_turn(&test, &server).await?;
    // Skip: no model change, but the step snapshot is already persisted.
    test.codex
        .submit(Op::StrategyFeedback {
            id: request.id,
            choice: StrategyFeedbackChoice::Skip,
        })
        .await?;
    test.codex.flush_rollout().await?;
    let rollout_path = test.codex.rollout_path().expect("rollout path");
    let snapshot = strategy_items(&rollout_path)?
        .last()
        .expect("step snapshot")
        .clone();
    let current = snapshot.model["current"]
        .as_str()
        .expect("restored model has an active node")
        .to_string();
    assert_eq!(current, "b");
    test.codex.submit(Op::Shutdown).await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::ShutdownComplete)
    })
    .await;

    // Resume from the rollout; the restored walk should pick up at node b and
    // inject node b's guidance into the next turn's model context.
    let resumed_cwd = test.config.cwd.clone();
    let mut resume_builder = test_codex()
        .with_home(Arc::clone(&home))
        .with_config(move |config| {
            config.cwd = resumed_cwd;
        });
    let resumed = resume_builder.resume(&server, home, rollout_path).await?;
    let resumed_mock = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            serde_json::from_slice::<serde_json::Value>(&request.body)
                .is_ok_and(|body| body.to_string().contains("continue the plan"))
        },
        sse(vec![
            ev_response_created("resp-resumed"),
            ev_completed("resp-resumed"),
        ]),
    )
    .await;
    resumed.submit_turn("continue the plan").await?;
    wait_for_event(&resumed.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    let request = resumed_mock.single_request();
    let body = request.body_json().to_string();
    assert!(
        body.contains("Strategy `test-strategy` — step `b`"),
        "restored strategy should resume at node b; body: {body}"
    );
    assert!(
        body.contains("then here"),
        "node b guidance should be injected after restore; body: {body}"
    );
    Ok(())
}

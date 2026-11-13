//! Round-trip tests: serialize a live model, restore it, and verify the
//! restored model behaves identically (including its RNG trajectory).

use pretty_assertions::assert_eq;

use crate::cond::ToolResultKind;
use crate::doc::parse_strategy;
use crate::model::StrategyModel;
use crate::model::StrategyTuning;
use crate::observe::ToolOutcome;
use crate::observe::TurnFeedback;
use crate::observe::TurnObservation;

const DOC_MAIN: &str = r#"
[strategy]
name = "main"
[origin]
a = 1.0
[node.a]
guidance = "ga"
tools = { "codegraph.impact" = 1.0, "shell" = 1.0 }
budget = 8
backtrack_to = "b"
[[node.a.edges]]
to = "b"
when = "tool_result:shell:error"
likelihood = 0.5
[[node.a.edges]]
to = "c"
when = "tool_result:codegraph.impact:nonempty"
likelihood = 0.5
[node.b]
guidance = "gb"
budget = 8
backtrack_to = "a"
[node.c]
guidance = "gc"
budget = 8
backtrack_to = "a"
"#;

fn model() -> StrategyModel {
    let doc = parse_strategy(DOC_MAIN).expect("parses");
    StrategyModel::new(doc, StrategyTuning::default())
}

fn observation() -> TurnObservation {
    TurnObservation {
        tools_called: vec!["codegraph.impact".to_string()],
        tool_results: vec![ToolOutcome::new(
            "codegraph.impact",
            ToolResultKind::NonEmpty,
        )],
        ..TurnObservation::default()
    }
}

#[test]
fn round_trip_preserves_state_exactly() {
    let mut original = model();
    let _ = original.enter();
    let _ = original.observe_turn(&observation());
    let _ = original.observe_turn(&observation());

    let json = serde_json::to_string(&original).expect("serializes");
    let restored: StrategyModel = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(restored, original);
}

#[test]
fn restored_model_behaves_identically() {
    let mut original = model();
    let _ = original.enter();
    let _ = original.observe_turn(&observation());

    let json = serde_json::to_string(&original).expect("serializes");
    let mut restored: StrategyModel = serde_json::from_str(&json).expect("deserializes");

    let original_outcome = original.observe_turn(&observation());
    let restored_outcome = restored.observe_turn(&observation());
    assert_eq!(restored_outcome, original_outcome);
    assert_eq!(restored, original);
}

#[test]
fn restored_rng_state_is_preserved() {
    // `rng` is serialized, so a restored model must not restart exploration
    // from a zero seed (which would trap xorshift64* and always pick the
    // first row entry).
    let mut original = model();
    let _ = original.enter();
    let _ = original.observe_turn(&TurnObservation::default());
    let _ = original.observe_turn(&TurnObservation::default());

    let json = serde_json::to_string(&original).expect("serializes");
    let mut restored: StrategyModel = serde_json::from_str(&json).expect("deserializes");

    let original_outcome = original.observe_turn(&TurnObservation::default());
    let restored_outcome = restored.observe_turn(&TurnObservation::default());
    assert_eq!(restored_outcome, original_outcome);
}

#[test]
fn apply_feedback_boosts_and_shrinks_used_tools() {
    let mut m = model();
    let _ = m.enter();

    let initial = *m
        .b_row("a")
        .expect("row")
        .get("codegraph.impact")
        .expect("tool");
    m.apply_feedback(&TurnFeedback {
        tools_called: vec!["codegraph.impact".to_string()],
        helpful: true,
    });
    let boosted = *m
        .b_row("a")
        .expect("row")
        .get("codegraph.impact")
        .expect("tool");
    assert!(boosted > initial, "helpful feedback boosts the tool weight");

    m.apply_feedback(&TurnFeedback {
        tools_called: vec!["codegraph.impact".to_string()],
        helpful: false,
    });
    let shrunk = *m
        .b_row("a")
        .expect("row")
        .get("codegraph.impact")
        .expect("tool");
    assert!(
        shrunk < boosted,
        "unhelpful feedback shrinks the tool weight"
    );
    assert!(
        shrunk < initial,
        "shrink overshoots below the neutral weight"
    );
}

#[test]
fn apply_feedback_enters_unknown_tools_at_neutral_weight() {
    let mut m = model();
    let _ = m.enter();
    assert!(
        m.b_row("a").expect("row").get("other.tool").is_none(),
        "other.tool is not authored in node a"
    );
    m.apply_feedback(&TurnFeedback {
        tools_called: vec!["other.tool".to_string()],
        helpful: false,
    });
    let weight = *m
        .b_row("a")
        .expect("row")
        .get("other.tool")
        .expect("entered at neutral weight");
    assert!(weight < 1.0, "unknown tool enters at 1.0 then shrinks");
}

#[test]
fn apply_feedback_ignores_inactive_or_unentered_model() {
    let mut m = model();
    // No enter(): no current node, so this must be a no-op.
    m.apply_feedback(&TurnFeedback {
        tools_called: vec!["codegraph.impact".to_string()],
        helpful: true,
    });
    assert!(m.current_node().is_none());
}

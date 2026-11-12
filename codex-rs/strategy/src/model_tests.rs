use pretty_assertions::assert_eq;

use crate::cond::ToolResultKind;
use crate::doc::BacktrackTarget;
use crate::doc::StrategyDoc;
use crate::doc::parse_strategy;
use crate::model::AdvanceOutcome;
use crate::model::BacktrackReason;
use crate::model::Phase;
use crate::model::StrategyModel;
use crate::model::StrategyTuning;
use crate::model::TransitionReason;
use crate::observe::MarkerStatus;
use crate::observe::StepMarker;
use crate::observe::ToolOutcome;
use crate::observe::TurnObservation;

const DOC_MAIN: &str = r#"
[strategy]
name = "main"
[origin]
a = 1.0
[node.a]
guidance = "ga"
tools = { "codegraph.impact" = 1.0, "shell" = 1.0 }
budget = 4
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
budget = 4
backtrack_to = "a"
[node.c]
guidance = "gc"
budget = 4
backtrack_to = "a"
"#;

const DOC_TERMINAL: &str = r#"
[strategy]
name = "terminal"
[origin]
a = 1.0
[node.a]
guidance = "ga"
budget = 4
[[node.a.edges]]
to = "done"
when = "tool_result:codegraph.impact:nonempty"
likelihood = 1.0
[node.done]
guidance = "gd"
budget = 1
terminal = true
"#;

const DOC_NO_EDGES: &str = r#"
[strategy]
name = "budget"
[origin]
a = 1.0
[node.a]
guidance = "ga"
budget = 2
backtrack_to = "b"
[node.b]
guidance = "gb"
budget = 2
backtrack_to = "origin"
"#;

fn model(doc: &str) -> StrategyModel {
    StrategyModel::new(
        parse_strategy(doc).expect("test doc parses"),
        StrategyTuning::default(),
    )
}

fn observation(results: &[(&str, ToolResultKind)]) -> TurnObservation {
    TurnObservation {
        tool_results: results
            .iter()
            .map(|(tool, kind)| ToolOutcome::new(*tool, *kind))
            .collect(),
        ..TurnObservation::default()
    }
}

#[test]
fn enter_picks_start_branch_from_pi() {
    let mut m = model(DOC_MAIN);
    let outcome = m.enter();
    assert_eq!(
        outcome,
        AdvanceOutcome::Advanced {
            from: "origin".into(),
            to: "a".into(),
            reason: TransitionReason::StartBranch,
        }
    );
    assert_eq!(m.current_node(), Some("a"));
    assert_eq!(m.phase(), Phase::Active);
    assert_eq!(m.visits().get("a"), Some(&1));
}

#[test]
fn same_seed_picks_same_branch() {
    let doc: StrategyDoc = parse_strategy(DOC_MAIN).expect("parses");
    let mut first = StrategyModel::new(doc.clone(), StrategyTuning::default());
    let mut second = StrategyModel::new(doc, StrategyTuning::default());
    first.set_seed(7);
    second.set_seed(7);
    let _ = first.enter();
    let _ = second.enter();
    assert_eq!(first.current_node(), second.current_node());
}

#[test]
fn start_branch_distribution_follows_pi() {
    let source = r#"
[strategy]
name = "two-branch"
[origin]
a = 6.0
b = 4.0
[node.a]
budget = 1
[node.b]
budget = 1
"#;
    let mut a_count = 0;
    for seed in 1..=200u64 {
        let mut m = StrategyModel::new(
            parse_strategy(source).expect("parses"),
            StrategyTuning::default(),
        );
        m.set_seed(seed);
        let _ = m.enter();
        if m.current_node() == Some("a") {
            a_count += 1;
        }
    }
    assert!(
        (90..=170).contains(&a_count),
        "branch `a` (π=0.6) should dominate over 200 seeds, got {a_count}"
    );
}

#[test]
fn condition_edge_ships_to_another_node() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    let outcome = m.observe_turn(&observation(&[(
        "codegraph.impact",
        ToolResultKind::NonEmpty,
    )]));
    assert_eq!(
        outcome,
        AdvanceOutcome::Advanced {
            from: "a".into(),
            to: "c".into(),
            reason: TransitionReason::Condition,
        }
    );
}

#[test]
fn strengthen_taken_edge_on_good_reward() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    let initial = m.row("a").to_vec();
    let _ = m.observe_turn(&observation(&[(
        "codegraph.impact",
        ToolResultKind::NonEmpty,
    )]));
    let updated = m.row("a").to_vec();
    let c_before = initial.iter().find(|(to, _)| to == "c").map(|(_, w)| *w);
    let c_after = updated.iter().find(|(to, _)| to == "c").map(|(_, w)| *w);
    assert!(c_after > c_before, "good reward strengthens the taken edge");
    let row_sum: f32 = updated.iter().map(|(_, w)| w).sum();
    assert!((row_sum - 1.0).abs() < 1e-5, "row renormalizes");
}

#[test]
fn weaken_taken_edge_on_bad_reward() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    let initial = m.row("a").to_vec();
    let _ = m.observe_turn(&observation(&[("shell", ToolResultKind::Error)]));
    let updated = m.row("a").to_vec();
    let b_before = initial.iter().find(|(to, _)| to == "b").map(|(_, w)| *w);
    let b_after = updated.iter().find(|(to, _)| to == "b").map(|(_, w)| *w);
    assert!(b_after < b_before, "bad reward weakens the taken edge");
}

#[test]
fn non_empty_results_grow_tool_weights() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    let before = *m
        .b_row("a")
        .expect("row")
        .get("codegraph.impact")
        .expect("tool");
    let _ = m.observe_turn(&observation(&[(
        "codegraph.impact",
        ToolResultKind::NonEmpty,
    )]));
    let after = *m
        .b_row("a")
        .expect("row")
        .get("codegraph.impact")
        .expect("tool");
    assert!(after > before, "non-empty result boosts the tool weight");
    assert_eq!(
        *m.b_row("a").expect("row").get("shell").expect("tool"),
        1.0,
        "uncalled tools untouched"
    );
}

#[test]
fn error_results_shrink_tool_weights() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    let before = *m.b_row("a").expect("row").get("shell").expect("tool");
    let _ = m.observe_turn(&observation(&[("shell", ToolResultKind::Error)]));
    let after = *m.b_row("a").expect("row").get("shell").expect("tool");
    assert!(after < before, "error result suppresses the tool weight");
}

#[test]
fn budget_expiry_backtracks_to_node() {
    let mut m = model(DOC_NO_EDGES);
    let _ = m.enter();
    let _ = m.observe_turn(&TurnObservation::default());
    let outcome = m.observe_turn(&TurnObservation::default());
    assert_eq!(
        outcome,
        AdvanceOutcome::Backtracked {
            from: "a".into(),
            to: BacktrackTarget::Node("b".into()),
            reason: BacktrackReason::BudgetExceeded,
        }
    );
    assert_eq!(m.current_node(), Some("b"));
}

#[test]
fn budget_expiry_backtracks_to_origin_and_reenters() {
    let mut m = model(DOC_NO_EDGES);
    let _ = m.enter();
    let _ = m.observe_turn(&TurnObservation::default());
    let _ = m.observe_turn(&TurnObservation::default()); // a → b
    let _ = m.observe_turn(&TurnObservation::default());
    let outcome = m.observe_turn(&TurnObservation::default()); // b → origin → re-enter
    assert_eq!(
        outcome,
        AdvanceOutcome::Backtracked {
            from: "b".into(),
            to: BacktrackTarget::Origin,
            reason: BacktrackReason::BudgetExceeded,
        }
    );
    assert!(
        m.current_node().is_some(),
        "origin backtrack re-enters a branch"
    );
}

#[test]
fn done_marker_advances_to_named_node() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    let obs = TurnObservation {
        marker: Some(StepMarker {
            node: None,
            status: MarkerStatus::Done,
            next: Some("c".into()),
            back_to: None,
            note: None,
        }),
        ..TurnObservation::default()
    };
    let outcome = m.observe_turn(&obs);
    assert_eq!(
        outcome,
        AdvanceOutcome::Advanced {
            from: "a".into(),
            to: "c".into(),
            reason: TransitionReason::MarkerNext,
        }
    );
}

#[test]
fn done_marker_into_terminal_completes() {
    let mut m = model(DOC_TERMINAL);
    let _ = m.enter();
    let obs = TurnObservation {
        marker: Some(StepMarker {
            node: None,
            status: MarkerStatus::Done,
            next: Some("done".into()),
            back_to: None,
            note: None,
        }),
        ..TurnObservation::default()
    };
    let outcome = m.observe_turn(&obs);
    assert_eq!(
        outcome,
        AdvanceOutcome::Complete {
            node: "done".into()
        }
    );
    assert_eq!(m.phase(), Phase::Complete);
    // Completed harness no longer transitions.
    assert_eq!(
        m.observe_turn(&TurnObservation::default()),
        AdvanceOutcome::Stayed {
            node: "done".into()
        }
    );
}

#[test]
fn wrong_marker_backtracks_to_named_node() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    let obs = TurnObservation {
        marker: Some(StepMarker {
            node: None,
            status: MarkerStatus::Wrong,
            next: None,
            back_to: Some("b".into()),
            note: Some("wrong path".into()),
        }),
        ..TurnObservation::default()
    };
    let outcome = m.observe_turn(&obs);
    assert_eq!(
        outcome,
        AdvanceOutcome::Backtracked {
            from: "a".into(),
            to: BacktrackTarget::Node("b".into()),
            reason: BacktrackReason::MarkerWrong,
        }
    );
}

#[test]
fn wrong_marker_without_target_uses_node_backtrack_to() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    let obs = TurnObservation {
        marker: Some(StepMarker {
            node: None,
            status: MarkerStatus::Wrong,
            next: None,
            back_to: None,
            note: None,
        }),
        ..TurnObservation::default()
    };
    let outcome = m.observe_turn(&obs);
    assert_eq!(
        outcome,
        AdvanceOutcome::Backtracked {
            from: "a".into(),
            to: BacktrackTarget::Node("b".into()),
            reason: BacktrackReason::MarkerWrong,
        }
    );
}

#[test]
fn visit_cap_declares_model_stuck() {
    let tuning = StrategyTuning {
        max_visits_per_node: 2,
        ..StrategyTuning::default()
    };
    let mut m = StrategyModel::new(parse_strategy(DOC_NO_EDGES).expect("parses"), tuning);
    let _ = m.enter();
    // Each node burns its budget (2 turns) before backtracking; eventually a
    // re-entry exceeds the per-node visit cap.
    let mut outcome = m.observe_turn(&TurnObservation::default());
    for _ in 0..24 {
        outcome = m.observe_turn(&TurnObservation::default());
        if outcome == AdvanceOutcome::Stuck {
            break;
        }
    }
    assert_eq!(outcome, AdvanceOutcome::Stuck);
    assert_eq!(m.phase(), Phase::Stuck);
}

#[test]
fn transition_cap_declares_model_stuck() {
    let tuning = StrategyTuning {
        max_transitions: 1,
        ..StrategyTuning::default()
    };
    let mut m = StrategyModel::new(parse_strategy(DOC_MAIN).expect("parses"), tuning);
    let _ = m.enter();
    let outcome = m.observe_turn(&observation(&[(
        "codegraph.impact",
        ToolResultKind::NonEmpty,
    )]));
    assert_eq!(outcome, AdvanceOutcome::Stuck);
}

#[test]
fn manual_backtrack_to_node_and_origin() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    let outcome = m.manual_backtrack(&BacktrackTarget::Node("b".into()));
    assert_eq!(
        outcome,
        AdvanceOutcome::Backtracked {
            from: "a".into(),
            to: BacktrackTarget::Node("b".into()),
            reason: BacktrackReason::Manual,
        }
    );
    let outcome = m.manual_backtrack(&BacktrackTarget::Origin);
    assert_eq!(
        outcome,
        AdvanceOutcome::Backtracked {
            from: "b".into(),
            to: BacktrackTarget::Origin,
            reason: BacktrackReason::Manual,
        }
    );
    assert!(m.current_node().is_some());
}

#[test]
fn manual_backtrack_to_unknown_node_stays() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    let outcome = m.manual_backtrack(&BacktrackTarget::Node("ghost".into()));
    assert_eq!(outcome, AdvanceOutcome::Stayed { node: "a".into() });
}

#[test]
fn prefer_tools_boosts_and_inserts() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    m.prefer_tools(&["codegraph.impact".into(), "codegraph.files".into()]);
    let row = m.b_row("a").expect("row");
    assert_eq!(*row.get("codegraph.impact").expect("existing tool"), 1.6);
    assert_eq!(*row.get("codegraph.files").expect("inserted tool"), 1.6);
}

#[test]
fn strong_prior_tools_follow_threshold() {
    let mut m = model(DOC_MAIN);
    let _ = m.enter();
    assert!(
        m.strong_prior_tools().is_empty(),
        "weights at 1.0 are not strong priors"
    );
    m.prefer_tools(&["codegraph.impact".into()]);
    assert_eq!(m.strong_prior_tools(), vec!["codegraph.impact".to_string()]);
}

#[test]
fn observation_before_enter_activates() {
    let mut m = model(DOC_MAIN);
    let outcome = m.observe_turn(&TurnObservation::default());
    assert!(matches!(
        outcome,
        AdvanceOutcome::Advanced {
            reason: TransitionReason::StartBranch,
            ..
        }
    ));
}

#[test]
fn guidance_and_tool_weights_follow_current_node() {
    let mut m = model(DOC_MAIN);
    assert_eq!(m.guidance(), None);
    assert_eq!(m.tool_weights(), None);
    let _ = m.enter();
    assert_eq!(m.guidance(), Some("ga"));
    assert!(m.tool_weights().is_some());
}

#[test]
fn bare_tool_keys_match_qualified_observed_keys() {
    use crate::observe::tool_key_matches;
    assert!(tool_key_matches("codegraph.impact", "codegraph.impact"));
    assert!(tool_key_matches("codegraph.impact", "impact"));
    assert!(tool_key_matches("server/tool", "tool"));
    assert!(tool_key_matches("CODEGRAPH.IMPACT", "codegraph.impact"));
    assert!(!tool_key_matches("codegraph.flow", "codegraph.impact"));
    assert!(!tool_key_matches("shell", "codegraph.impact"));
}

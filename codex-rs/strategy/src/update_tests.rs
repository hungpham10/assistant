use pretty_assertions::assert_eq;

use crate::cond::ToolResultKind;
use crate::observe::MarkerStatus;
use crate::observe::StepMarker;
use crate::observe::ToolOutcome;
use crate::observe::TurnObservation;
use crate::update::clamp_a_weight;
use crate::update::clamp_b_weight;
use crate::update::ema;
use crate::update::normalize_in_place;
use crate::update::observation_reward;

fn obs(results: &[ToolResultKind], tokens_used: u64) -> TurnObservation {
    TurnObservation {
        tool_results: results
            .iter()
            .enumerate()
            .map(|(i, kind)| ToolOutcome::new(format!("tool{i}"), *kind))
            .collect(),
        tokens_used,
        ..TurnObservation::default()
    }
}

#[test]
fn markers_override_everything() {
    let mut o = obs(&[ToolResultKind::NonEmpty], 0);
    o.marker = Some(StepMarker {
        node: None,
        status: MarkerStatus::Done,
        next: None,
        back_to: None,
        note: None,
    });
    assert_eq!(observation_reward(&o, 20_000), 1.0);
    o.marker = Some(StepMarker {
        node: None,
        status: MarkerStatus::Wrong,
        next: None,
        back_to: None,
        note: None,
    });
    assert_eq!(observation_reward(&o, 20_000), 0.0);
}

#[test]
fn base_reward_classes() {
    assert_eq!(
        observation_reward(&obs(&[], 0), 20_000),
        0.3,
        "no results is neutral"
    );
    assert_eq!(
        observation_reward(&obs(&[ToolResultKind::NonEmpty], 0), 20_000),
        0.7,
        "any non-empty result scores high"
    );
    assert_eq!(
        observation_reward(&obs(&[ToolResultKind::Empty], 0), 20_000),
        0.15,
        "all-empty scores low"
    );
    assert_eq!(
        observation_reward(
            &obs(&[ToolResultKind::Error, ToolResultKind::Empty], 0),
            20_000
        ),
        0.05,
        "errors score lowest"
    );
    assert_eq!(
        observation_reward(
            &obs(&[ToolResultKind::Error, ToolResultKind::NonEmpty], 0),
            20_000
        ),
        0.7,
        "non-empty wins over errors"
    );
}

#[test]
fn token_efficiency_adds_bounded_bonus() {
    let under = observation_reward(&obs(&[ToolResultKind::NonEmpty], 5_000), 20_000);
    let over = observation_reward(&obs(&[ToolResultKind::NonEmpty], 40_000), 20_000);
    assert_eq!(under, 0.7 + (1.0 - 0.25) * 0.2);
    assert_eq!(over, 0.7, "over-budget turns get no bonus");
    assert_eq!(
        observation_reward(&obs(&[ToolResultKind::NonEmpty], 0), 20_000),
        0.7,
        "unknown token usage adds nothing"
    );
    assert_eq!(
        observation_reward(&obs(&[ToolResultKind::NonEmpty], 10_000), 0),
        0.7,
        "zero budget disables the token component"
    );
}

#[test]
fn reward_is_clamped() {
    assert_eq!(
        observation_reward(&obs(&[ToolResultKind::NonEmpty], 0), 20_000),
        0.7
    );
    assert!(observation_reward(&obs(&[], 0), 20_000) >= 0.0);
    assert!(observation_reward(&obs(&[], 0), 20_000) <= 1.0);
}

#[test]
fn ema_moves_toward_observation() {
    assert_eq!(ema(0.5, 1.0, 0.2), 0.8 * 0.5 + 0.2 * 1.0);
    assert_eq!(ema(0.5, 0.0, 0.2), 0.8 * 0.5);
    assert_eq!(ema(0.5, 0.5, 0.2), 0.5);
}

#[test]
fn clamps_hold_bounds() {
    assert_eq!(clamp_a_weight(0.0), 0.02);
    assert_eq!(clamp_a_weight(1.5), 0.95);
    assert_eq!(clamp_a_weight(0.5), 0.5);
    assert_eq!(clamp_b_weight(0.01), 0.05);
    assert_eq!(clamp_b_weight(10.0), 3.0);
    assert_eq!(clamp_b_weight(1.2), 1.2);
}

#[test]
fn normalize_keeps_order_and_sums_to_one() {
    let mut weights = vec![("a".to_string(), 3.0), ("b".to_string(), 1.0)];
    normalize_in_place(&mut weights);
    assert_eq!(weights[0].0, "a");
    assert!((weights[0].1 - 0.75).abs() < 1e-6);
    assert!((weights[1].1 - 0.25).abs() < 1e-6);
}

#[test]
fn normalize_survives_degenerate_input() {
    let mut weights = vec![("a".to_string(), 0.0)];
    normalize_in_place(&mut weights);
    assert_eq!(weights[0].1, 0.0, "all-zero rows are left untouched");
}

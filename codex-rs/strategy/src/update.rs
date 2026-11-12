//! Pure reward and update math for the strategy model.
//!
//! The update rule is deliberately small and bounded: an EMA toward the
//! observed reward for transition likelihoods (A rows, probabilities), and
//! bounded multiplicative nudges for tool-preference weights (B rows, which
//! are weights around 1.0 rather than probabilities).

use crate::cond::ToolResultKind;
use crate::observe::MarkerStatus;
use crate::observe::TurnObservation;

/// Bounds for A-row entries (probabilities within a normalized row).
const A_WEIGHT_MIN: f32 = 0.02;
const A_WEIGHT_MAX: f32 = 0.95;

/// Bounds for B-row entries (tool-preference weights around 1.0).
const B_WEIGHT_MIN: f32 = 0.05;
const B_WEIGHT_MAX: f32 = 3.0;

/// Scores a turn in `[0, 1]`.
///
/// - `Done` markers score 1.0, `Wrong` markers score 0.0 (evidence with the
///   final say).
/// - Otherwise the base reward comes from the tool-result classes: any
///   non-empty result scores high, all-empty scores low, errors score
///   lower still, and a turn with no classified results is neutral.
/// - Staying under the token budget adds a small efficiency bonus.
pub fn observation_reward(obs: &TurnObservation, token_budget: u64) -> f32 {
    if let Some(marker) = &obs.marker {
        return match marker.status {
            MarkerStatus::Done => 1.0,
            MarkerStatus::Wrong => 0.0,
        };
    }
    let mut reward = base_reward_from_results(obs);
    if token_budget > 0 && obs.tokens_used > 0 {
        let ratio = (obs.tokens_used as f32 / token_budget as f32).clamp(0.0, 1.0);
        reward += (1.0 - ratio) * 0.2;
    }
    reward.clamp(0.0, 1.0)
}

fn base_reward_from_results(obs: &TurnObservation) -> f32 {
    if obs.tool_results.is_empty() {
        return 0.3;
    }
    let has_nonempty = obs
        .tool_results
        .iter()
        .any(|result| result.kind == ToolResultKind::NonEmpty);
    let has_error = obs
        .tool_results
        .iter()
        .any(|result| result.kind == ToolResultKind::Error);
    if has_nonempty {
        0.7
    } else if has_error {
        0.05
    } else {
        0.15
    }
}

/// One EMA step: `L' = (1 - α) · L + α · r`.
pub fn ema(current: f32, reward: f32, alpha: f32) -> f32 {
    (1.0 - alpha) * current + alpha * reward
}

/// Clamps a transition-likelihood entry before the row is renormalized.
pub fn clamp_a_weight(weight: f32) -> f32 {
    weight.clamp(A_WEIGHT_MIN, A_WEIGHT_MAX)
}

/// Clamps a tool-preference weight.
pub fn clamp_b_weight(weight: f32) -> f32 {
    weight.clamp(B_WEIGHT_MIN, B_WEIGHT_MAX)
}

/// Normalizes `(key, weight)` pairs in place so weights sum to 1 while
/// keeping insertion order.
pub fn normalize_in_place(weights: &mut [(String, f32)]) {
    let total: f32 = weights.iter().map(|(_, w)| w).sum();
    if total > 0.0 && total.is_finite() {
        for (_, w) in weights.iter_mut() {
            *w /= total;
        }
    }
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;

//! The strategy state machine: π/A/B live model, transitions, and guards.
//!
//! `StrategyModel` owns the live (mutable) parameters initialized by the
//! [`StrategyDoc`](crate::doc::StrategyDoc): the start distribution π, one
//! transition row per node (A), and tool-preference rows (B). Turn
//! observations drive transitions and bounded reward updates (the `update`
//! module holds the pure math). Guards keep the loop finite: per-node visit
//! caps and a total transition cap turn the harness "stuck", at which point
//! steering stops.

use crate::cond::ToolResultKind;
use crate::doc::BacktrackTarget;
use crate::doc::StrategyDoc;
use crate::observe::MarkerStatus;
use crate::observe::StepMarker;
use crate::observe::TurnFeedback;
use crate::observe::TurnObservation;
use crate::observe::tool_key_matches;
use crate::update::clamp_a_weight;
use crate::update::clamp_b_weight;
use crate::update::ema;
use crate::update::normalize_in_place;
use crate::update::observation_reward;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

/// Runtime tuning knobs, independent of the authored document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyTuning {
    /// EMA learning rate α for likelihood updates.
    pub learning_rate: f32,
    /// Probability of picking a non-dominant branch (exploration).
    pub explore_epsilon: f32,
    /// Tool weights at or above this value are treated as strong priors
    /// (candidates for force-advertising).
    pub strong_prior_weight: f32,
    /// Per-node visit cap; exceeding it on entry declares the model stuck.
    pub max_visits_per_node: u32,
    /// Total transition cap for the whole session.
    pub max_transitions: u32,
    /// Token budget used by the reward function when a node has none.
    pub default_token_budget: u64,
}

impl Default for StrategyTuning {
    fn default() -> Self {
        Self {
            learning_rate: 0.2,
            explore_epsilon: 0.1,
            strong_prior_weight: 1.5,
            max_visits_per_node: 3,
            max_transitions: 40,
            default_token_budget: 20_000,
        }
    }
}

/// Lifecycle of the harness within a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Steering is active.
    Active,
    /// A terminal node was reached; steering stops.
    Complete,
    /// A guard fired; steering stops.
    Stuck,
}

/// Result of one harness step (activation, observation, or manual backtrack).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AdvanceOutcome {
    Stayed {
        node: String,
    },
    Advanced {
        from: String,
        to: String,
        reason: TransitionReason,
    },
    Backtracked {
        from: String,
        to: BacktrackTarget,
        reason: BacktrackReason,
    },
    Complete {
        node: String,
    },
    Stuck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionReason {
    /// The start branch was picked from π.
    StartBranch,
    /// A conditional edge fired ("the tool ships to another node").
    Condition,
    /// A `Done` marker named the next node explicitly.
    MarkerNext,
    /// Weighted (epsilon-greedy) choice over the node's row.
    Weighted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BacktrackReason {
    /// A `Wrong` marker requested the backtrack.
    MarkerWrong,
    /// The node's turn budget expired without progress.
    BudgetExceeded,
    /// The user requested the backtrack (`/plan back`).
    Manual,
}

/// Accumulated usage statistics per node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeStats {
    pub turns: u32,
    pub tokens: u64,
    pub duration_ms: u64,
}

/// Result of entering a node, before it is framed as an [`AdvanceOutcome`].
enum NodeEntry {
    Advanced(String),
    Complete(String),
    Stuck,
}

/// Live strategy model over a validated [`StrategyDoc`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyModel {
    doc: StrategyDoc,
    tuning: StrategyTuning,
    /// Live start distribution, normalized.
    pi: Vec<(String, f32)>,
    /// Live transition rows per node, aggregated by target and normalized.
    rows: HashMap<String, Vec<(String, f32)>>,
    /// Live tool-preference rows per node.
    b: HashMap<String, HashMap<String, f32>>,
    current: Option<String>,
    phase: Phase,
    visits: HashMap<String, u32>,
    turns_in_node: u32,
    total_transitions: u32,
    /// Persisted so a restored model continues the same exploration
    /// trajectory; a zero seed would trap xorshift64* at 0 forever.
    rng: u64,
    stats: HashMap<String, NodeStats>,
}

impl StrategyModel {
    pub fn new(doc: StrategyDoc, tuning: StrategyTuning) -> Self {
        let mut rows = HashMap::new();
        let mut b = HashMap::new();
        for (id, node) in &doc.nodes {
            let mut aggregated: Vec<(String, f32)> = Vec::new();
            for edge in &node.edges {
                if let Some(entry) = aggregated.iter_mut().find(|(to, _)| to == &edge.to) {
                    entry.1 += edge.likelihood;
                } else {
                    aggregated.push((edge.to.clone(), edge.likelihood));
                }
            }
            normalize_in_place(&mut aggregated);
            rows.insert(id.clone(), aggregated);
            b.insert(id.clone(), node.tools.clone());
        }
        let pi = doc.origin.clone();
        let rng = seed_from_name(&doc.meta.name);
        Self {
            doc,
            tuning,
            pi,
            rows,
            b,
            current: None,
            phase: Phase::Active,
            visits: HashMap::new(),
            turns_in_node: 0,
            total_transitions: 0,
            rng,
            stats: HashMap::new(),
        }
    }

    /// Set RNG seed for testing.
    pub fn set_seed(&mut self, seed: u64) {
        self.rng = seed;
    }

    pub fn doc(&self) -> &StrategyDoc {
        &self.doc
    }

    pub fn tuning(&self) -> &StrategyTuning {
        &self.tuning
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn current_node(&self) -> Option<&str> {
        self.current.as_deref()
    }

    pub fn guidance(&self) -> Option<&str> {
        self.current
            .as_ref()
            .and_then(|n| self.doc.node(n))
            .map(|n| n.guidance.as_str())
    }

    pub fn tool_weights(&self) -> Option<&HashMap<String, f32>> {
        self.current.as_ref().and_then(|n| self.b.get(n))
    }

    pub fn strong_prior_tools(&self) -> Vec<String> {
        self.tool_weights()
            .map(|weights| {
                weights
                    .iter()
                    .filter(|(_, w)| **w >= self.tuning.strong_prior_weight)
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns visit count for a specific node.
    pub fn visits(&self, node: &str) -> u32 {
        self.visits.get(node).copied().unwrap_or(0)
    }

    /// Returns all visit counts (for backward compat with tests).
    pub fn all_visits(&self) -> &HashMap<String, u32> {
        &self.visits
    }

    pub fn turns_in_node(&self) -> u32 {
        self.turns_in_node
    }

    pub fn stats(&self, node: &str) -> Option<NodeStats> {
        self.stats.get(node).copied()
    }

    /// Returns the A-row (transition likelihoods) for a node.
    pub fn row(&self, node: &str) -> Option<&Vec<(String, f32)>> {
        self.rows.get(node)
    }

    /// Returns the B-row (tool preferences) for a node.
    pub fn b_row(&self, node: &str) -> Option<&HashMap<String, f32>> {
        self.b.get(node)
    }

    /// Observe a completed turn and advance the model.
    pub fn observe_turn(&mut self, obs: &TurnObservation) -> AdvanceOutcome {
        // If not active, return appropriate outcome.
        if self.phase != Phase::Active {
            return match self.phase {
                Phase::Stuck => AdvanceOutcome::Stuck,
                _ => self.stayed(),
            };
        }

        // If no current node, pick start branch.
        let Some(node_id) = self.current.clone() else {
            return self.enter();
        };
        self.turns_in_node += 1;
        let stats = self.stats.entry(node_id.clone()).or_default();
        stats.turns += 1;
        stats.tokens += obs.tokens_used;
        stats.duration_ms += obs.duration_ms;

        let token_budget = self
            .doc
            .node(&node_id)
            .and_then(|node| node.token_budget)
            .unwrap_or(self.tuning.default_token_budget);
        let reward = observation_reward(obs, token_budget);

        // Markers are evidence with the final say on transitions.
        if let Some(marker) = &obs.marker {
            return match marker.status {
                MarkerStatus::Done => self.advance_after_done_marker(&node_id, marker, reward, obs),
                MarkerStatus::Wrong => self.backtrack_after_wrong_marker(&node_id, marker, obs),
            };
        }

        // Conditional edges fire first: a tool outcome ships the decision to
        // another node.
        if let Some(target) = self.first_matching_edge(&node_id, obs) {
            return self.transition(&node_id, &target, TransitionReason::Condition, reward, obs);
        }

        // Budget expiry backtracks before any weighted move is considered.
        if let Some(node) = self.doc.node(&node_id)
            && self.turns_in_node >= node.budget
        {
            let target = node.backtrack_to.clone();
            return self.backtrack(&node_id, target, BacktrackReason::BudgetExceeded, obs);
        }

        let row = self.rows.get(&node_id).cloned().unwrap_or_default();
        if row.is_empty() {
            return self.stayed();
        }
        let target = if self.next_rand() < self.tuning.explore_epsilon {
            let idx = uniform_pick(&mut self.rng, row.len());
            row[idx].0.clone()
        } else {
            let idx = weighted_pick(&mut self.rng, &row);
            row[idx].0.clone()
        };
        self.transition(&node_id, &target, TransitionReason::Weighted, reward, obs)
    }

    /// Explicitly pick the start branch from π (used for first turn).
    pub fn enter(&mut self) -> AdvanceOutcome {
        if self.current.is_some() {
            return self.stayed();
        }
        let (branch, entry) = self.enter_from_origin();
        entry.frame_from_origin(&branch)
    }

    /// Manual backtrack requested by the user (`/plan back <node|origin>`).
    pub fn manual_backtrack(&mut self, target: &BacktrackTarget) -> AdvanceOutcome {
        if self.phase != Phase::Active {
            return match self.phase {
                Phase::Stuck => AdvanceOutcome::Stuck,
                _ => self.stayed(),
            };
        }
        let Some(node_id) = self.current.clone() else {
            return self.enter();
        };
        let resolved = match target {
            BacktrackTarget::Origin => BacktrackTarget::Origin,
            BacktrackTarget::Node(id) if self.doc.nodes.contains_key(id) => {
                BacktrackTarget::Node(id.clone())
            }
            // Unknown target: stay where we are.
            BacktrackTarget::Node(_) => return self.stayed(),
        };
        let from = node_id;
        let outcome = match &resolved {
            BacktrackTarget::Origin => {
                self.apply_tool_nudges(&from, &TurnObservation::default());
                self.current = None;
                self.turns_in_node = 0;
                let (branch, entry) = self.enter_from_origin();
                entry.frame_from_origin(&branch)
            }
            BacktrackTarget::Node(node) => self
                .enter_node(node)
                .frame_advanced(&from, TransitionReason::Weighted),
        };
        outcome.frame_backtracked(from, resolved, BacktrackReason::Manual)
    }

    /// Boost tool weights for the current node (used for `/plan prefer`).
    pub fn prefer_tools(&mut self, tools: &[String]) {
        if self.phase != Phase::Active {
            return;
        }
        let Some(node_id) = self.current.clone() else {
            return;
        };
        let boost = 1.0 + self.tuning.learning_rate * 3.0;
        let Some(row) = self.b.get_mut(&node_id) else {
            return;
        };
        for tool in tools {
            let entry = row.entry(tool.clone()).or_insert(1.0);
            *entry = clamp_b_weight(*entry * boost);
        }
    }

    /// Applies explicit user feedback on a finished turn by nudging the
    /// current node's B-row weights for the tools the turn used. Missing
    /// entries enter at the neutral weight `1.0` before the nudge, matching
    /// [`StrategyModel::prefer_tools`].
    pub fn apply_feedback(&mut self, feedback: &TurnFeedback) {
        if self.phase != Phase::Active {
            return;
        }
        let Some(node_id) = self.current.clone() else {
            return;
        };
        let Some(row) = self.b.get_mut(&node_id) else {
            return;
        };
        let step = self.tuning.learning_rate;
        for tool in &feedback.tools_called {
            let matched = row
                .keys()
                .find(|authored| tool_key_matches(tool, authored))
                .cloned()
                .unwrap_or_else(|| tool.clone());
            let entry = row.entry(matched).or_insert(1.0);
            *entry = if feedback.helpful {
                clamp_b_weight(*entry * (1.0 + step))
            } else {
                clamp_b_weight(*entry * (1.0 - step))
            };
        }
    }

    // -- internals ----------------------------------------------------------

    fn advance_after_done_marker(
        &mut self,
        node_id: &str,
        marker: &StepMarker,
        reward: f32,
        obs: &TurnObservation,
    ) -> AdvanceOutcome {
        if let Some(next) = marker
            .next
            .as_deref()
            .filter(|next| self.doc.nodes.contains_key(*next))
        {
            let next = next.to_string();
            return self.transition(node_id, &next, TransitionReason::MarkerNext, reward, obs);
        }
        // No (valid) explicit target: fall through to the regular decision.
        if let Some(target) = self.first_matching_edge(node_id, obs) {
            return self.transition(node_id, &target, TransitionReason::Condition, reward, obs);
        }
        let row = self.rows.get(node_id).cloned().unwrap_or_default();
        if row.is_empty() {
            return self.stayed();
        }
        let idx = weighted_pick(&mut self.rng, &row);
        let target = row[idx].0.clone();
        self.transition(node_id, &target, TransitionReason::Weighted, reward, obs)
    }

    fn backtrack_after_wrong_marker(
        &mut self,
        node_id: &str,
        marker: &StepMarker,
        obs: &TurnObservation,
    ) -> AdvanceOutcome {
        let fallback = self
            .doc
            .node(node_id)
            .map(|n| n.backtrack_to.clone())
            .unwrap_or(BacktrackTarget::Origin);
        let target = match &marker.back_to {
            Some(back) if back == "origin" => BacktrackTarget::Origin,
            Some(back) if self.doc.nodes.contains_key(back) => BacktrackTarget::Node(back.clone()),
            Some(_) | None => fallback,
        };
        let from = node_id.to_string();
        let outcome = match &target {
            BacktrackTarget::Origin => {
                self.apply_tool_nudges(&from, obs);
                self.current = None;
                self.turns_in_node = 0;
                let (branch, entry) = self.enter_from_origin();
                entry.frame_from_origin(&branch)
            }
            BacktrackTarget::Node(node) => self
                .enter_node(node)
                .frame_advanced(&from, TransitionReason::Weighted),
        };
        outcome.frame_backtracked(from, target, BacktrackReason::MarkerWrong)
    }

    fn first_matching_edge(&self, node_id: &str, obs: &TurnObservation) -> Option<String> {
        self.doc.node(node_id).and_then(|node| {
            node.edges.iter().find_map(|edge| {
                edge.when
                    .as_ref()
                    .filter(|cond| cond.matches(obs, self.turns_in_node))
                    .map(|_| edge.to.clone())
            })
        })
    }

    fn transition(
        &mut self,
        from: &str,
        to: &str,
        reason: TransitionReason,
        reward: f32,
        obs: &TurnObservation,
    ) -> AdvanceOutcome {
        // Reward the row we are leaving: strengthen the edge toward the
        // target when the turn scored well, weaken it when it did not.
        self.update_row(from, to, reward);
        self.apply_tool_nudges(from, obs);
        self.enter_node(to).frame_advanced(from, reason)
    }

    fn backtrack(
        &mut self,
        from: &str,
        to: BacktrackTarget,
        reason: BacktrackReason,
        obs: &TurnObservation,
    ) -> AdvanceOutcome {
        self.apply_tool_nudges(from, obs);
        let from = from.to_string();
        let outcome = match &to {
            BacktrackTarget::Origin => {
                self.current = None;
                self.turns_in_node = 0;
                let (branch, entry) = self.enter_from_origin();
                entry.frame_from_origin(&branch)
            }
            BacktrackTarget::Node(node) => self
                .enter_node(node)
                .frame_advanced(&from, TransitionReason::Weighted),
        };
        outcome.frame_backtracked(from, to, reason)
    }

    fn enter_from_origin(&mut self) -> (String, NodeEntry) {
        let branch = if self.next_rand() < self.tuning.explore_epsilon {
            let idx = uniform_pick(&mut self.rng, self.pi.len());
            self.pi[idx].0.clone()
        } else {
            let idx = weighted_pick(&mut self.rng, &self.pi);
            self.pi[idx].0.clone()
        };
        let entry = self.enter_node(&branch);
        (branch, entry)
    }

    fn enter_node(&mut self, node: &str) -> NodeEntry {
        if !self.doc.nodes.contains_key(node) {
            return NodeEntry::Advanced(self.current.clone().unwrap_or_default());
        }
        if self.total_transitions >= self.tuning.max_transitions
            || self.visits.get(node).copied().unwrap_or(0) >= self.tuning.max_visits_per_node
        {
            self.phase = Phase::Stuck;
            return NodeEntry::Stuck;
        }
        *self.visits.entry(node.to_string()).or_insert(0) += 1;
        self.current = Some(node.to_string());
        self.turns_in_node = 0;
        self.total_transitions += 1;
        if self.doc.node(node).is_some_and(|n| n.terminal) {
            self.phase = Phase::Complete;
            return NodeEntry::Complete(node.to_string());
        }
        NodeEntry::Advanced(node.to_string())
    }

    fn stayed(&self) -> AdvanceOutcome {
        AdvanceOutcome::Stayed {
            node: self.current.clone().unwrap_or_default(),
        }
    }

    fn update_row(&mut self, node: &str, target: &str, reward: f32) {
        let Some(row) = self.rows.get_mut(node) else {
            return;
        };
        for (to, weight) in row.iter_mut() {
            if to == target {
                *weight = clamp_a_weight(ema(*weight, reward, self.tuning.learning_rate));
            }
        }
        normalize_in_place(row);
    }

    fn apply_tool_nudges(&mut self, node: &str, obs: &TurnObservation) {
        let alpha = self.tuning.learning_rate;
        let Some(row) = self.b.get_mut(node) else {
            return;
        };
        for outcome in &obs.tool_results {
            let Some(weight) = row
                .iter_mut()
                .find(|(tool, _)| tool_key_matches(&outcome.tool, tool))
                .map(|(_, weight)| weight)
            else {
                continue;
            };
            *weight = match outcome.kind {
                ToolResultKind::NonEmpty => clamp_b_weight(*weight * (1.0 + alpha * 0.5)),
                ToolResultKind::Empty => clamp_b_weight(*weight * (1.0 - alpha * 0.5)),
                ToolResultKind::Error => clamp_b_weight(*weight * (1.0 - alpha)),
            };
        }
    }

    fn next_rand(&mut self) -> f32 {
        next_rand(&mut self.rng)
    }
}

/// Draws a uniform sample in `[0, 1)` from the xorshift64* state.
fn next_rand(rng: &mut u64) -> f32 {
    *rng ^= *rng >> 12;
    *rng ^= *rng << 25;
    *rng ^= *rng >> 27;
    let v = rng.wrapping_mul(0x2545_F491_4F6C_DD1D);
    (v >> 11) as f32 / (1u64 << 53) as f32
}

/// Weighted pick over a normalized row; returns the chosen index.
fn weighted_pick(rng: &mut u64, entries: &[(String, f32)]) -> usize {
    if entries.is_empty() {
        return 0;
    }
    let total: f32 = entries.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        return 0;
    }
    let r = next_rand(rng) * total;
    let mut acc = 0.0;
    for (idx, (_, w)) in entries.iter().enumerate() {
        acc += w;
        if r <= acc {
            return idx;
        }
    }
    entries.len() - 1
}

/// Uniform pick over `len` entries; returns the chosen index.
fn uniform_pick(rng: &mut u64, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let idx = (next_rand(rng) * len as f32) as usize;
    idx.min(len - 1)
}

impl NodeEntry {
    fn frame_from_origin(self, branch: &str) -> AdvanceOutcome {
        match self {
            NodeEntry::Advanced(_) => AdvanceOutcome::Advanced {
                from: "origin".into(),
                to: branch.to_string(),
                reason: TransitionReason::StartBranch,
            },
            NodeEntry::Complete(node) => AdvanceOutcome::Complete { node },
            NodeEntry::Stuck => AdvanceOutcome::Stuck,
        }
    }

    fn frame_advanced(self, from: &str, reason: TransitionReason) -> AdvanceOutcome {
        match self {
            NodeEntry::Advanced(to) => AdvanceOutcome::Advanced {
                from: from.to_string(),
                to,
                reason,
            },
            NodeEntry::Complete(node) => AdvanceOutcome::Complete { node },
            NodeEntry::Stuck => AdvanceOutcome::Stuck,
        }
    }
}

impl AdvanceOutcome {
    /// Reframes an entry outcome as an explicit backtrack. `Complete` and
    /// `Stuck` pass through unchanged.
    fn frame_backtracked(
        self,
        from: String,
        to: BacktrackTarget,
        reason: BacktrackReason,
    ) -> AdvanceOutcome {
        match self {
            Self::Advanced { .. } => Self::Backtracked { from, to, reason },
            other => other,
        }
    }
}

fn seed_from_name(name: &str) -> u64 {
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    for byte in name.bytes() {
        seed ^= byte as u64;
        seed = seed.wrapping_mul(0x100_0000_01B3);
    }
    if seed == 0 {
        0x9E37_79B9_7F4A_7C15
    } else {
        seed
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;

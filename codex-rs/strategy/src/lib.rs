//! Strategy harness: prior-initialized probabilistic way-of-working models.
//!
//! A strategy is initialized from a TOML document that plays the role of
//! initialization data for a small Bayesian/Markov model of how a task should
//! be approached:
//!
//! - `origin` holds the start distribution π over entry branches.
//! - every node carries a transition row (edges with conditions and
//!   likelihoods), a tool-preference row (weights that later multiply the
//!   Jev tool-selection percentiles), and per-step budgets.
//! - as turns run, observations (tool outcomes, token usage, user feedback)
//!   update the likelihoods with a bounded EMA-style rule, so the way of
//!   working adapts without any history rewrite.
//!
//! Everything in the document is a *suggestion*: the model steers softly and
//! the driving agent remains free to deviate. Guards (per-node visit caps,
//! transition caps) keep the loop bounded.

pub mod cond;
pub mod doc;
pub mod model;
pub mod observe;
pub mod update;

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod persistence_tests;

pub use cond::ConditionParseError;
pub use cond::StrategyCondition;
pub use cond::ToolResultKind;
pub use doc::BacktrackTarget;
pub use doc::StrategyDoc;
pub use doc::StrategyEdge;
pub use doc::StrategyMeta;
pub use doc::StrategyNode;
pub use doc::StrategyParseError;
pub use doc::builtin_strategy_source;
pub use doc::builtin_strategy_sources;
pub use doc::parse_strategy;
pub use model::AdvanceOutcome;
pub use model::BacktrackReason;
pub use model::NodeStats;
pub use model::Phase;
pub use model::StrategyModel;
pub use model::StrategyTuning;
pub use model::TransitionReason;
pub use observe::MarkerStatus;
pub use observe::StepMarker;
pub use observe::ToolOutcome;
pub use observe::TurnFeedback;
pub use observe::TurnObservation;
pub use observe::tool_key_matches;

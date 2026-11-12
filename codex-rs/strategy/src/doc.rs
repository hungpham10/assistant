//! Strategy documents: TOML initialization data for the harness model.
//!
//! A document defines three initialization blocks for the probabilistic model:
//!
//! - `[origin]` — the start distribution π over entry branches.
//! - `[node.<id>]` — one decision node: guidance text, tool-preference
//!   weights (the B row), turn/token budgets, backtrack target, and its
//!   transition edges (the A row).
//!
//! All likelihoods are normalized at load time; validation rejects unknown
//! references and non-positive weights up front so the runtime never has to
//! re-check them.

use crate::cond::StrategyCondition;
use crate::update::normalize_in_place;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

/// Parses and validates a strategy from its TOML source.
pub fn parse_strategy(source: &str) -> Result<StrategyDoc, StrategyParseError> {
    let raw: StrategyDocRaw =
        toml::from_str(source).map_err(|err| StrategyParseError::Toml(err.to_string()))?;
    StrategyDoc::try_from_raw(raw)
}

/// Name of the built-in explore strategy (bind → search/trace → judge).
pub const EXPLORE_CODE_STRATEGY: &str = "explore-code";
/// Name of the built-in writing strategy (consult docs → draft → verify).
pub const WRITE_CODE_STRATEGY: &str = "write-code";

const EXPLORE_CODE_SOURCE: &str = include_str!("strategies/explore-code.toml");
const WRITE_CODE_SOURCE: &str = include_str!("strategies/write-code.toml");

/// Every built-in strategy as `(name, toml source)` pairs.
pub fn builtin_strategy_sources() -> Vec<(&'static str, &'static str)> {
    vec![
        (EXPLORE_CODE_STRATEGY, EXPLORE_CODE_SOURCE),
        (WRITE_CODE_STRATEGY, WRITE_CODE_SOURCE),
    ]
}

/// Source for one built-in strategy by name.
pub fn builtin_strategy_source(name: &str) -> Option<&'static str> {
    builtin_strategy_sources()
        .into_iter()
        .find(|(builtin, _)| *builtin == name)
        .map(|(_, source)| source)
}

/// Validated and normalized strategy document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyDoc {
    pub meta: StrategyMeta,
    /// Start distribution π over node ids, normalized to sum to 1.
    pub origin: Vec<(String, f32)>,
    pub nodes: BTreeMap<String, StrategyNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyMeta {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyNode {
    #[serde(default)]
    pub guidance: String,
    /// Tool-preference weights (B row). Not probabilities: values around 1.0
    /// are neutral, above boost, below suppress.
    pub tools: BTreeMap<String, f32>,
    /// Turns allowed in this node before the auto-backtrack fires.
    pub budget: u32,
    /// Soft token budget for the node, used by the reward function.
    pub token_budget: Option<u64>,
    pub backtrack_to: BacktrackTarget,
    /// Transition row (A row), normalized to sum to 1.
    pub edges: Vec<StrategyEdge>,
    /// Entering a terminal node completes the strategy.
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyEdge {
    pub to: String,
    pub when: Option<StrategyCondition>,
    /// Normalized share of this edge within its node's row.
    pub likelihood: f32,
}

/// Where a backtrack returns to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BacktrackTarget {
    /// The strategy start (re-picks a branch from π, visits reset).
    Origin,
    /// A specific earlier node.
    Node(String),
}

impl BacktrackTarget {
    /// Resolves the human-authored form: `None` or `"origin"` mean the start.
    fn from_authored(value: Option<&str>) -> Self {
        match value {
            None | Some("origin") => Self::Origin,
            Some(node) => Self::Node(node.to_string()),
        }
    }
}

impl fmt::Display for BacktrackTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Origin => write!(f, "origin"),
            Self::Node(node) => write!(f, "{node}"),
        }
    }
}

/// Errors surfaced while parsing or validating a strategy document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StrategyParseError {
    #[error("invalid strategy TOML: {0}")]
    Toml(String),
    #[error("node `{0}`: {1}")]
    Node(String, String),
    #[error("origin: {0}")]
    Origin(String),
    #[error("strategy: {0}")]
    Meta(String),
}

// Raw serde shapes mirroring the authored TOML.

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyDocRaw {
    strategy: StrategyMetaRaw,
    origin: BTreeMap<String, f32>,
    node: BTreeMap<String, StrategyNodeRaw>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyMetaRaw {
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyNodeRaw {
    #[serde(default)]
    guidance: String,
    #[serde(default)]
    tools: BTreeMap<String, f32>,
    #[serde(default = "default_budget")]
    budget: u32,
    #[serde(default)]
    token_budget: Option<u64>,
    #[serde(default)]
    backtrack_to: Option<String>,
    #[serde(default)]
    edges: Vec<StrategyEdgeRaw>,
    #[serde(default)]
    terminal: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyEdgeRaw {
    to: String,
    #[serde(default)]
    when: Option<String>,
    #[serde(default = "default_likelihood")]
    likelihood: f32,
}

fn default_budget() -> u32 {
    4
}

fn default_likelihood() -> f32 {
    1.0
}

impl StrategyDoc {
    fn try_from_raw(raw: StrategyDocRaw) -> Result<Self, StrategyParseError> {
        if raw.strategy.name.trim().is_empty() {
            return Err(StrategyParseError::Meta("name must not be empty".into()));
        }
        if raw.node.is_empty() {
            return Err(StrategyParseError::Meta(
                "at least one node is required".into(),
            ));
        }
        if raw.node.contains_key("origin") {
            return Err(StrategyParseError::Meta(
                "`origin` is reserved for the start distribution and cannot be a node id".into(),
            ));
        }

        let node_ids: std::collections::HashSet<String> = raw.node.keys().cloned().collect();
        let origin = normalize_origin(&raw.origin, &node_ids)?;
        let nodes = raw
            .node
            .into_iter()
            .map(|(id, node)| Ok((id.clone(), normalize_node(&id, node, &node_ids)?)))
            .collect::<Result<BTreeMap<_, _>, StrategyParseError>>()?;

        Ok(Self {
            meta: StrategyMeta {
                name: raw.strategy.name,
                description: raw.strategy.description,
            },
            origin,
            nodes,
        })
    }

    /// Lookup of a single node by id.
    pub fn node(&self, id: &str) -> Option<&StrategyNode> {
        self.nodes.get(id)
    }
}

fn normalize_origin(
    origin: &BTreeMap<String, f32>,
    node_ids: &std::collections::HashSet<String>,
) -> Result<Vec<(String, f32)>, StrategyParseError> {
    if origin.is_empty() {
        return Err(StrategyParseError::Origin(
            "at least one branch likelihood is required".into(),
        ));
    }
    let mut normalized: Vec<(String, f32)> = Vec::with_capacity(origin.len());
    for (id, weight) in origin {
        if !node_ids.contains(id) {
            return Err(StrategyParseError::Origin(format!(
                "branch `{id}` does not match any node"
            )));
        }
        if *weight <= 0.0 || !weight.is_finite() {
            return Err(StrategyParseError::Origin(format!(
                "branch `{id}` must have a positive finite likelihood"
            )));
        }
        normalized.push((id.clone(), *weight));
    }
    normalize_in_place(&mut normalized);
    Ok(normalized)
}

fn normalize_node(
    id: &str,
    node: StrategyNodeRaw,
    node_ids: &std::collections::HashSet<String>,
) -> Result<StrategyNode, StrategyParseError> {
    let node_err = |msg: String| StrategyParseError::Node(id.to_string(), msg);

    if node.budget == 0 {
        return Err(node_err("budget must be at least 1 turn".into()));
    }
    for (tool, weight) in &node.tools {
        if *weight <= 0.0 || !weight.is_finite() {
            return Err(node_err(format!(
                "tool `{tool}` must have a positive finite weight"
            )));
        }
    }
    match &node.backtrack_to {
        None => {}
        Some(target) if target == "origin" || node_ids.contains(target) => {}
        Some(target) => {
            return Err(node_err(format!(
                "backtrack target `{target}` does not match any node"
            )));
        }
    }

    let mut edges: Vec<StrategyEdge> = Vec::with_capacity(node.edges.len());
    for edge in node.edges {
        if !node_ids.contains(&edge.to) {
            return Err(node_err(format!(
                "edge target `{}` does not match any node",
                edge.to
            )));
        }
        if edge.likelihood <= 0.0 || !edge.likelihood.is_finite() {
            return Err(node_err(format!(
                "edge to `{}` must have a positive finite likelihood",
                edge.to
            )));
        }
        let when = edge
            .when
            .map(|raw| StrategyCondition::parse(&raw).map_err(|err| node_err(err.to_string())))
            .transpose()?;
        edges.push(StrategyEdge {
            to: edge.to,
            when,
            likelihood: edge.likelihood,
        });
    }
    // The A row is the normalized fallback distribution over outgoing edges.
    let mut row: Vec<(String, f32)> = edges
        .iter()
        .map(|edge| (edge.to.clone(), edge.likelihood))
        .collect();
    normalize_in_place(&mut row);
    for (edge, (to, likelihood)) in edges.iter_mut().zip(row.iter()) {
        edge.to = to.clone();
        edge.likelihood = *likelihood;
    }

    Ok(StrategyNode {
        guidance: node.guidance,
        tools: node.tools,
        budget: node.budget,
        token_budget: node.token_budget,
        backtrack_to: BacktrackTarget::from_authored(node.backtrack_to.as_deref()),
        edges,
        terminal: node.terminal,
    })
}

#[cfg(test)]
#[path = "doc_tests.rs"]
mod tests;

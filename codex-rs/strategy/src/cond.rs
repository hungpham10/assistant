//! Condition DSL evaluated against per-turn facts.
//!
//! Conditions are authored as plain strings in strategy files, e.g.
//! `tool_result:codegraph.trace_full_flow:nonempty`. They are parsed once at
//! load time and kept in typed form afterwards.

use crate::observe::TurnObservation;
use crate::observe::tool_key_matches;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;

/// Outcome class of a single tool call result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolResultKind {
    /// The call completed but produced nothing usable.
    Empty,
    /// The call produced content.
    NonEmpty,
    /// The call failed.
    Error,
}

/// A transition condition over bounded per-turn facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StrategyCondition {
    /// `<tool>` was called during the turn.
    ToolCalled(String),
    /// `<tool>` was called and its result classifies as the given kind.
    ToolResult {
        tool: String,
        outcome: ToolResultKind,
    },
    /// At least `<n>` turns have run in the current node.
    TurnCountGe(u32),
    /// The user text for the turn contains `<text>` (case-insensitive).
    UserTextContains(String),
    /// Always matches.
    Always,
}

impl StrategyCondition {
    /// Evaluates the condition against the turn's facts. `turns_in_node` is
    /// the count of turns already spent in the current node (inclusive of
    /// the turn being evaluated).
    pub fn matches(&self, obs: &TurnObservation, turns_in_node: u32) -> bool {
        match self {
            Self::Always => true,
            Self::ToolCalled(tool) => obs
                .tools_called
                .iter()
                .any(|called| tool_key_matches(called, tool)),
            Self::ToolResult { tool, outcome } => obs
                .tool_results
                .iter()
                .any(|result| tool_key_matches(&result.tool, tool) && result.kind == *outcome),
            Self::TurnCountGe(n) => turns_in_node >= *n,
            Self::UserTextContains(text) => obs
                .user_text
                .as_deref()
                .is_some_and(|user| user.to_lowercase().contains(&text.to_lowercase())),
        }
    }

    /// Parses a condition from its string form.
    pub fn parse(raw: &str) -> Result<Self, ConditionParseError> {
        let raw_trimmed = raw.trim();
        let (kind, rest) = match raw_trimmed.split_once(':') {
            Some((kind, rest)) => (kind, rest),
            None => (raw_trimmed, ""),
        };
        match kind {
            "always" if rest.is_empty() => Ok(Self::Always),
            "tool_called" => {
                let tool = rest.trim();
                non_empty(tool, raw)
                    .map(|tool| Self::ToolCalled(tool.to_string()))
                    .map_err(|reason| ConditionParseError::new(raw, reason))
            }
            "tool_result" => {
                let (tool, outcome) = rest.split_once(':').ok_or_else(|| {
                    ConditionParseError::new(
                        raw,
                        "expected `tool_result:<tool>:empty|nonempty|error`",
                    )
                })?;
                let outcome = outcome.trim();
                let outcome = match outcome {
                    "empty" => ToolResultKind::Empty,
                    "nonempty" => ToolResultKind::NonEmpty,
                    "error" => ToolResultKind::Error,
                    other => {
                        return Err(ConditionParseError::new(
                            raw,
                            format!("unknown result kind `{other}`"),
                        ));
                    }
                };
                non_empty(tool.trim(), raw)
                    .map(|tool| Self::ToolResult {
                        tool: tool.to_string(),
                        outcome,
                    })
                    .map_err(|reason| ConditionParseError::new(raw, reason))
            }
            "turn_count_ge" => rest
                .trim()
                .parse::<u32>()
                .map(Self::TurnCountGe)
                .map_err(|_| {
                    ConditionParseError::new(raw, "expected `turn_count_ge:<positive integer>`")
                }),
            "user_text_contains" => non_empty(rest.trim(), raw)
                .map(|text| Self::UserTextContains(text.to_string()))
                .map_err(|reason| ConditionParseError::new(raw, reason)),
            other => Err(ConditionParseError::new(
                raw,
                format!(
                    "unknown condition kind `{other}`; expected one of always, tool_called, \
                     tool_result, turn_count_ge, user_text_contains"
                ),
            )),
        }
    }
}

impl fmt::Display for StrategyCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Always => write!(f, "always"),
            Self::ToolCalled(tool) => write!(f, "tool_called:{tool}"),
            Self::ToolResult { tool, outcome } => {
                let outcome = match outcome {
                    ToolResultKind::Empty => "empty",
                    ToolResultKind::NonEmpty => "nonempty",
                    ToolResultKind::Error => "error",
                };
                write!(f, "tool_result:{tool}:{outcome}")
            }
            Self::TurnCountGe(n) => write!(f, "turn_count_ge:{n}"),
            Self::UserTextContains(text) => write!(f, "user_text_contains:{text}"),
        }
    }
}

/// Error produced when a condition string cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid condition `{raw}`: {reason}")]
pub struct ConditionParseError {
    raw: String,
    reason: String,
}

impl ConditionParseError {
    fn new(raw: &str, reason: impl Into<String>) -> Self {
        Self {
            raw: raw.to_string(),
            reason: reason.into(),
        }
    }
}

fn non_empty<'a>(value: &'a str, raw: &str) -> Result<&'a str, String> {
    if value.is_empty() {
        Err(format!("`{raw}` requires a non-empty argument"))
    } else {
        Ok(value)
    }
}

#[cfg(test)]
#[path = "cond_tests.rs"]
mod tests;

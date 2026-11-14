use super::TurnError;
use crate::RequestId;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct DeprecationNoticeNotification {
    /// Concise summary of what is deprecated.
    pub summary: String,
    /// Optional extra guidance, such as migration steps or rationale.
    pub details: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct WarningNotification {
    /// Optional thread target when the warning applies to a specific thread.
    pub thread_id: Option<String>,
    /// Concise warning message for the user.
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct GuardianWarningNotification {
    /// Thread target for the guardian warning.
    pub thread_id: String,
    /// Concise guardian warning message for the user.
    pub message: String,
}

/// MCP tools Jev advertised directly for a turn (TUI tool-usage stats).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct JevToolSuggestionNotification {
    /// Optional thread target when the suggestion applies to a specific thread.
    pub thread_id: Option<String>,
    pub turn_id: String,
    /// Tools advertised directly for this turn, as `server/tool`.
    pub tools: Vec<String>,
    /// Strategy-adjusted probability per advertised tool, sorted descending.
    pub scores: Vec<(String, f32)>,
    /// `true` when Jev found no relevant tools.
    pub no_match: bool,
    /// Strategy name when a `/plan <strategy>` harness tuned the ranking.
    pub strategy: Option<String>,
    /// Strategy step (node) that was active for this turn.
    pub node: Option<String>,
    /// Human-readable reason Jev did not run this turn (fail-open skip).
    /// `None` when Jev produced a suggestion or is disabled in config.
    pub skip_reason: Option<String>,
}

/// Strategy-harness step transition for `/plan <strategy>` sessions.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct StrategyStepNotification {
    /// Optional thread target when the step applies to a specific thread.
    pub thread_id: Option<String>,
    /// Strategy name.
    pub strategy: String,
    /// Step (node) the harness moved to, when a step is active.
    pub node: Option<String>,
    /// Harness lifecycle: `active`, `complete`, or `stuck`.
    pub phase: String,
    /// Human-readable detail (transition reason, backtrack target, …).
    pub detail: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ErrorNotification {
    pub error: TurnError,
    // Set to true if the error is transient and the app-server process will automatically retry.
    // If true, this will not interrupt a turn.
    pub will_retry: bool,
    pub thread_id: String,
    pub turn_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ServerRequestResolvedNotification {
    pub thread_id: String,
    pub request_id: RequestId,
}

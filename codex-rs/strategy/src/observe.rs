//! Per-turn observations fed into the strategy model.

use crate::cond::ToolResultKind;
use serde::Deserialize;
use serde::Serialize;

/// One classified tool call from the turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOutcome {
    /// Tool key as authored in strategy files, e.g. `codegraph.impact` or a
    /// bare builtin name like `shell`.
    pub tool: String,
    pub kind: ToolResultKind,
}

impl ToolOutcome {
    pub fn new(tool: impl Into<String>, kind: ToolResultKind) -> Self {
        Self {
            tool: tool.into(),
            kind,
        }
    }
}

/// Optional `<step/>` marker the assistant may emit at the end of a turn.
///
/// Markers are *evidence*, not commands: `Done` scores the turn as a success
/// and its `next` attribute is honored when it names an existing node;
/// `Wrong` scores the turn as a failure and triggers the backtrack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepMarker {
    /// Node the marker refers to; `None` means "the current node".
    pub node: Option<String>,
    pub status: MarkerStatus,
    /// Explicit forward target for `Done` markers.
    pub next: Option<String>,
    /// Explicit backtrack target for `Wrong` markers; `None` falls back to
    /// the node's own `backtrack_to`.
    pub back_to: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkerStatus {
    Done,
    Wrong,
}

impl StepMarker {
    /// Extracts the first `<step .../>` (or `<step ...></step>`) marker from
    /// assistant text. Returns `None` when the text holds no marker.
    pub fn parse(text: &str) -> Option<Self> {
        let open = text.find("<step")?;
        let after_open = &text[open + "<step".len()..];
        let end = after_open.find('>')?;
        let (attrs, closing) = after_open.split_at(end);
        let is_self_closing = closing.starts_with("/>");
        if !is_self_closing && !closing.starts_with('>') {
            return None;
        }
        let mut marker = Self {
            node: None,
            status: MarkerStatus::Done,
            next: None,
            back_to: None,
            note: None,
        };
        marker.status = parse_attr(attrs, "status")
            .and_then(|raw| match raw.as_str() {
                "done" => Some(MarkerStatus::Done),
                "wrong" => Some(MarkerStatus::Wrong),
                _ => None,
            })
            .unwrap_or(MarkerStatus::Done);
        marker.node = parse_attr(attrs, "id");
        marker.next = parse_attr(attrs, "next");
        marker.back_to = parse_attr(attrs, "back");
        marker.note = parse_attr(attrs, "note");
        Some(marker)
    }
}

/// Extracts a quoted attribute value from the raw attribute slice.
fn parse_attr(attrs: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let idx = attrs.find(&needle)?;
    let after_eq = &attrs[idx + needle.len()..];
    let quote = after_eq.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &after_eq[1..];
    let end = value.find(quote)?;
    Some(value[..end].to_string())
}

/// Matches an observed tool key against an authored strategy key.
///
/// Authored keys may be fully qualified (`codegraph.flow`, `server/tool`) or
/// bare (`shell`); a bare key matches the last segment of a qualified key.
/// Matching is case-insensitive.
pub fn tool_key_matches(observed: &str, authored: &str) -> bool {
    if observed.eq_ignore_ascii_case(authored) {
        return true;
    }
    let qualified = authored.contains('.') || authored.contains(':') || authored.contains('/');
    if qualified {
        return false;
    }
    observed
        .rsplit(['.', ':', '/'])
        .next()
        .is_some_and(|last| last.eq_ignore_ascii_case(authored))
}

/// Everything the strategy model learns from a single turn.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TurnObservation {
    /// Tool keys called during the turn (duplicates allowed).
    pub tools_called: Vec<String>,
    /// Classified results for the calls made during the turn.
    pub tool_results: Vec<ToolOutcome>,
    /// Total tokens billed for the turn (input + output, when known).
    pub tokens_used: u64,
    /// Wall-clock duration of the turn in milliseconds (when known).
    pub duration_ms: u64,
    /// Lowercased user text for the turn, capped at a bounded size by the
    /// caller; used only for `user_text_contains` conditions.
    pub user_text: Option<String>,
    /// Optional assistant-emitted `<step/>` marker.
    pub marker: Option<StepMarker>,
}

#[cfg(test)]
#[path = "observe_tests.rs"]
mod tests;

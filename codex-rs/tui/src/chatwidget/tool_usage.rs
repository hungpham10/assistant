//! Per-tool usage statistics for the `/status` card.
//!
//! The tracker is fed from two places:
//!
//! * completed `ThreadItem`s (tool calls), recorded in
//!   [`ChatWidget::handle_thread_item`]; and
//! * Jev tool suggestions forwarded from core
//!   (`ServerNotification::JevToolSuggestion`), so `/status` can also show
//!   how often Jev advertised each tool directly.
//!
//! Only completed items are counted; in-progress items are skipped and
//! item ids are deduped so replay of a resumed thread does not double-count.

use std::collections::HashMap;
use std::collections::HashSet;

use chrono::DateTime;
use chrono::Local;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::McpToolCallStatus;
use codex_app_server_protocol::ThreadItem;

#[derive(Debug, Clone, Default)]
pub(crate) struct ToolUsageEntry {
    pub(crate) name: String,
    pub(crate) calls: u64,
    pub(crate) errors: u64,
    pub(crate) jev_suggestions: u64,
    pub(crate) last_used_at: Option<DateTime<Local>>,
    pub(crate) last_turn_id: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct ToolUsageTracker {
    entries: HashMap<String, ToolUsageEntry>,
    seen_item_ids: HashSet<String>,
}

impl ToolUsageTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Record one completed tool call. `item_id` dedupes replays.
    pub(crate) fn record_call(
        &mut self,
        name: impl Into<String>,
        item_id: impl Into<String>,
        errored: bool,
        turn_id: impl Into<String>,
    ) {
        let name = name.into();
        let item_id = item_id.into();
        if !self.seen_item_ids.insert(item_id) {
            return;
        }
        let turn_id = turn_id.into();
        let entry = self
            .entries
            .entry(name.clone())
            .or_insert_with(|| ToolUsageEntry {
                name,
                ..ToolUsageEntry::default()
            });
        entry.calls = entry.calls.saturating_add(1);
        if errored {
            entry.errors = entry.errors.saturating_add(1);
        }
        entry.last_used_at = Some(Local::now());
        if !turn_id.is_empty() {
            entry.last_turn_id = Some(turn_id);
        }
    }

    /// Record one Jev suggestion round for the advertised tool names.
    pub(crate) fn record_jev_suggestions(&mut self, tools: &[String]) {
        for name in tools {
            let key = name.clone();
            let entry = self
                .entries
                .entry(key.clone())
                .or_insert_with(|| ToolUsageEntry {
                    name: key,
                    ..ToolUsageEntry::default()
                });
            entry.jev_suggestions = entry.jev_suggestions.saturating_add(1);
        }
    }

    /// Top `n` tools by call count (ties: more Jev suggestions first).
    pub(crate) fn top(&self, n: usize) -> Vec<&ToolUsageEntry> {
        let mut sorted: Vec<&ToolUsageEntry> = self.entries.values().collect();
        sorted.sort_by(|a, b| {
            b.calls
                .cmp(&a.calls)
                .then_with(|| b.jev_suggestions.cmp(&a.jev_suggestions))
                .then_with(|| a.name.cmp(&b.name))
        });
        sorted.truncate(n);
        sorted
    }

    /// Tools Jev suggested at least once but were never called.
    pub(crate) fn suggested_but_unused(&self, n: usize) -> Vec<&ToolUsageEntry> {
        let mut sorted: Vec<&ToolUsageEntry> = self
            .entries
            .values()
            .filter(|entry| entry.jev_suggestions > 0 && entry.calls == 0)
            .collect();
        sorted.sort_by(|a, b| {
            b.jev_suggestions
                .cmp(&a.jev_suggestions)
                .then_with(|| a.name.cmp(&b.name))
        });
        sorted.truncate(n);
        sorted
    }

    /// Look up a tool by the name the user typed after `/status`.
    /// Accepts exact keys (`server/tool`), full qualified names
    /// (`mcp:server/tool`), or a bare tool name (`tool`).
    pub(crate) fn lookup(&self, query: &str) -> Option<&ToolUsageEntry> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        if let Some(entry) = self.entries.get(query) {
            return Some(entry);
        }
        // Normalize `namespace:server/tool` (Jev style) to `server/tool`.
        let normalized = query.rsplit(':').next().unwrap_or(query);
        if let Some(entry) = self.entries.get(normalized) {
            return Some(entry);
        }
        // Bare tool name: match the part after the last `/`.
        if !normalized.contains('/') {
            let mut matches: Vec<&ToolUsageEntry> = Vec::new();
            for entry in self.entries.values() {
                let bare = entry.name.rsplit('/').next().unwrap_or(&entry.name);
                if bare.eq_ignore_ascii_case(normalized) {
                    matches.push(entry);
                }
            }
            if matches.len() == 1 {
                return Some(matches[0]);
            }
            if let Some(first) = matches.first() {
                return Some(first);
            }
        }
        None
    }
}

/// Extract `(tool display name, item id, errored)` from a `ThreadItem`,
/// or `None` when the item is not a tool call or is still in progress.
pub(crate) fn extract_tool_call(item: &ThreadItem) -> Option<(String, String, bool)> {
    match item {
        ThreadItem::CommandExecution {
            id,
            status,
            exit_code,
            ..
        } => {
            if *status == CommandExecutionStatus::InProgress {
                return None;
            }
            let errored = *status == CommandExecutionStatus::Failed
                || exit_code.is_some_and(|code| code != 0);
            Some(("shell".to_string(), id.clone(), errored))
        }
        ThreadItem::McpToolCall {
            id,
            server,
            tool,
            status,
            error,
            ..
        } => {
            if *status == McpToolCallStatus::InProgress {
                return None;
            }
            let errored = error.is_some() || *status == McpToolCallStatus::Failed;
            Some((format!("{server}/{tool}"), id.clone(), errored))
        }
        ThreadItem::WebSearch(item) => Some(("web_search".to_string(), item.id.clone(), false)),
        ThreadItem::ImageGeneration(item) => Some((
            "image_generation".to_string(),
            item.id.clone(),
            item.status == "failed",
        )),
        ThreadItem::DynamicToolCall {
            id,
            namespace,
            tool,
            ..
        } => {
            let name = match namespace {
                Some(namespace) => format!("{namespace}/{tool}"),
                None => tool.clone(),
            };
            Some((name, id.clone(), false))
        }
        ThreadItem::CollabAgentToolCall { id, tool, .. } => {
            let name = format!("{:?}", tool).to_lowercase();
            Some((name, id.clone(), false))
        }
        ThreadItem::FileChange { id, .. } => Some(("apply_patch".to_string(), id.clone(), false)),
        ThreadItem::ImageView { id: _, path } => {
            Some(("view_image".to_string(), path.clone().to_string(), false))
        }
        ThreadItem::Sleep { id, .. } => Some(("sleep".to_string(), id.clone(), false)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tool_usage_tests.rs"]
mod tests;

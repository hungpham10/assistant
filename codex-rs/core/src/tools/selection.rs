use std::collections::HashSet;

use codex_protocol::ToolName;

/// Per-session tool enable/disable overrides controlled by the user through
/// the TUI `/tools` command. Overrides only affect what the model sees up
/// front: a demoted tool becomes `Deferred` (discoverable through
/// `tool_search`), never unregistered.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionToolSelection {
    /// Tools the user disabled for this session.
    deny: HashSet<String>,
    /// Tools the user re-enabled, overriding session deny entries.
    enable: HashSet<String>,
}

/// Builtin tools that write to the workspace or execute commands; demoted in
/// Plan mode where only read-only, non-destructive tools make sense.
pub(crate) const PLAN_MODE_DEMOTED_TOOLS: &[&str] = &[
    "shell",
    "unified_exec",
    "apply_patch",
    "Write",
    "write",
    "Edit",
    "edit",
    "Bash",
    "bash",
];

/// Tools that are always advertised regardless of selection, because the
/// collaboration modes and approval flows depend on them.
pub(crate) const PROTECTED_TOOLS: &[&str] = &["update_plan", "request_user_input"];

impl SessionToolSelection {
    /// Sets one tool's enabled state. `key` is either a plain tool name
    /// (`apply_patch`) or a namespace wildcard (`server:*`).
    pub(crate) fn set_enabled(&mut self, key: impl Into<String>, enabled: bool) {
        let key = key.into();
        self.enable.remove(&key);
        self.deny.remove(&key);
        if enabled {
            self.enable.insert(key);
        } else {
            self.deny.insert(key);
        }
    }

    /// Decides whether a tool should be demoted from the initial
    /// model-visible list. `plan_mode` applies the read-only builtin rule.
    pub(crate) fn tool_is_demoted(&self, name: &ToolName, plan_mode: bool) -> bool {
        let display = display_key(name);
        if protected(name) {
            return false;
        }
        if self.enable.contains(&display) {
            return false;
        }
        if self.deny.contains(&display) {
            return true;
        }
        if let Some(namespace) = name.namespace.as_deref()
            && self.deny.contains(&format!("{namespace}:*"))
        {
            return true;
        }
        plan_mode
            && name.namespace.is_none()
            && PLAN_MODE_DEMOTED_TOOLS.contains(&name.name.as_str())
    }
}

/// Canonical string form used by overrides: `namespace:name`, or just `name`.
pub(crate) fn display_key(name: &ToolName) -> String {
    match &name.namespace {
        Some(namespace) => format!("{namespace}:{}", name.name),
        None => name.name.clone(),
    }
}

fn protected(name: &ToolName) -> bool {
    name.namespace.is_none() && PROTECTED_TOOLS.contains(&name.name.as_str())
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;

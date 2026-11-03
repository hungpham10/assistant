use std::collections::HashSet;

use codex_protocol::ToolName;

/// Per-session tool enable/disable overrides controlled by the user through
/// the TUI `/tools` command. Overrides allow demoting tools to `Deferred`
/// (discoverable through `tool_search`) or hiding them entirely from the
/// model's initial tool list.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionToolSelection {
    /// Tools the user disabled for this session.
    deny: HashSet<String>,
    /// Tools the user re-enabled, overriding session deny entries.
    enable: HashSet<String>,
    /// Tools the user hid entirely from this session (not advertised and
    /// not discoverable via `tool_search`).
    hide: HashSet<String>,
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
        self.set_hidden(key, !enabled);
    }

    /// Hides or shows a tool entirely for this session. Hidden tools are
    /// removed from the model-visible list and from dispatch.
    pub(crate) fn set_hidden(&mut self, key: impl Into<String>, hidden: bool) {
        let key = key.into();
        // Protected tools cannot be hidden.
        let display = key.clone();
        if let Some(namespace) = display.split_once(':') {
            let name = ToolName::namespaced(namespace.0, namespace.1);
            if protected(&name) {
                self.enable.remove(&display);
                self.deny.remove(&display);
                self.hide.remove(&display);
                self.enable.insert(display);
                return;
            }
        } else {
            let name = ToolName::plain(display.as_str());
            if protected(&name) {
                self.enable.remove(&display);
                self.deny.remove(&display);
                self.hide.remove(&display);
                self.enable.insert(display);
                return;
            }
        }
        self.enable.remove(&key);
        self.deny.remove(&key);
        self.hide.remove(&key);
        if !hidden {
            self.enable.insert(key);
        } else {
            self.hide.insert(key);
        }
    }

    /// Decides whether a tool should be hidden entirely from this session.
    pub(crate) fn tool_is_hidden(&self, name: &ToolName) -> bool {
        if protected(name) {
            return false;
        }
        let display = display_key(name);
        if self.enable.contains(&display) {
            return false;
        }
        if self.hide.contains(&display) {
            return true;
        }
        if let Some(namespace) = name.namespace.as_deref()
            && self.hide.contains(&format!("{namespace}:*"))
        {
            return true;
        }
        false
    }

    /// Decides whether a tool should be demoted from the initial
    /// model-visible list. `plan_mode` applies the read-only builtin rule.
    pub(crate) fn tool_is_demoted(&self, name: &ToolName, plan_mode: bool) -> bool {
        if self.tool_is_hidden(name) {
            return false;
        }
        let display = display_key(name);
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

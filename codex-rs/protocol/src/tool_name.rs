use serde::Deserialize;
use serde::Serialize;
use std::cmp::Ordering;
use std::fmt;

/// Identifies a callable tool, preserving the namespace split when the model
/// provides one.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ToolName {
    pub name: String,
    pub namespace: Option<String>,
}

impl ToolName {
    pub fn new(namespace: Option<String>, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            namespace,
        }
    }

    /// Builds the flat wire name for a namespaced tool, e.g.
    /// `mcp__codex_apps__pets` + `_adopt` → `mcp__codex_apps__pets_adopt`.
    ///
    /// Mirrors the namespace flattening used by the chat-wire-compat converter
    /// so that tool names advertised under Claude Code / native wires always
    /// match the alias registered in `ToolRegistry` and the name the MCP
    /// server actually advertises.
    pub fn flat_namespaced(namespace: impl Into<String>, name: impl Into<String>) -> String {
        let namespace = namespace.into();
        let name = name.into();
        if namespace.ends_with('_') || name.starts_with('_') {
            format!("{namespace}{name}")
        } else {
            format!("{namespace}_{name}")
        }
    }

    pub fn plain(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            namespace: None,
        }
    }

    pub fn namespaced(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            namespace: Some(namespace.into()),
        }
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.namespace {
            Some(namespace) => write!(f, "{namespace}{}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

impl Ord for ToolName {
    fn cmp(&self, other: &Self) -> Ordering {
        let lhs = match &self.namespace {
            Some(namespace) => (namespace.as_str(), Some(self.name.as_str())),
            None => (self.name.as_str(), None),
        };
        let rhs = match &other.namespace {
            Some(namespace) => (namespace.as_str(), Some(other.name.as_str())),
            None => (other.name.as_str(), None),
        };
        lhs.cmp(&rhs)
    }
}

impl PartialOrd for ToolName {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl From<String> for ToolName {
    fn from(name: String) -> Self {
        Self::plain(name)
    }
}

impl From<&str> for ToolName {
    fn from(name: &str) -> Self {
        Self::plain(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_namespaced_inserts_underscore_by_default() {
        assert_eq!(
            ToolName::flat_namespaced("mcp__codegraph", "search_symbol"),
            "mcp__codegraph_search_symbol"
        );
        assert_eq!(
            ToolName::flat_namespaced("mcp__codex_apps__pets", "adopt"),
            "mcp__codex_apps__pets_adopt"
        );
    }

    #[test]
    fn flat_namespaced_avoids_double_underscore() {
        // namespace already ends with '_' or name starts with '_' → no extra '_'.
        assert_eq!(
            ToolName::flat_namespaced("mcp__codex_apps__pets_", "adopt"),
            "mcp__codex_apps__pets_adopt"
        );
        assert_eq!(
            ToolName::flat_namespaced("mcp__codex_apps__pets", "_adopt"),
            "mcp__codex_apps__pets_adopt"
        );
    }
}

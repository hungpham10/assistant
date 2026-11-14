use super::ContextualUserFragment;

/// Hard cap on the rendered hint body so the injected fragment stays bounded
/// regardless of how many tools were advertised or how long their names are.
const MAX_HINT_CHARS: usize = 1500;

/// Turn-scoped hint listing the tools Jev picked for this question in priority
/// order, so the model calls them first and reaches for anything else through
/// `tool_search`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JevToolPriorityHint {
    /// Model-visible tool names (the exact name in the tool spec the model
    /// sees), in Jev priority order. Probability is kept for stable ordering
    /// but is not rendered — raw scores read as low confidence and discourage
    /// tool use.
    names: Vec<String>,
}

impl JevToolPriorityHint {
    /// `names` must already be sorted by descending Jev score.
    pub(crate) fn new(names: Vec<String>) -> Self {
        Self { names }
    }
}

impl ContextualUserFragment for JevToolPriorityHint {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<jev_tool_priority>", "</jev_tool_priority>")
    }

    fn body(&self) -> String {
        let mut text = String::from(
            "Tool routing for this turn — call these tools first, in this order of preference:\n",
        );
        let mut truncated = false;
        for (index, name) in self.names.iter().enumerate() {
            let line = format!("{}. `{name}`\n", index + 1);
            if text.len() + line.len() + "</jev_tool_priority>".len() > MAX_HINT_CHARS {
                truncated = true;
                break;
            }
            text.push_str(&line);
        }
        if truncated {
            text.push_str("- (list truncated)\n");
        }
        text.push_str(
            "Every other tool is NOT attached this turn; to use another tool, first call `tool_search`.\n",
        );
        format!("\n{text}")
    }
}

#[cfg(test)]
#[path = "jev_tool_priority_hint_tests.rs"]
mod tests;

use super::ContextualUserFragment;

/// Hard cap on the rendered hint body so the injected fragment stays bounded
/// regardless of how many tools were advertised or how long their names are.
const MAX_HINT_CHARS: usize = 1500;

/// Turn-scoped hint listing the tools Jev advertised for this question with
/// their strategy-adjusted probability, so the model prefers them when calling.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JevToolPriorityHint {
    /// Tool names in `server/tool` form with probability in `[0, 1]`, sorted
    /// descending by the caller.
    scores: Vec<(String, f32)>,
}

impl JevToolPriorityHint {
    pub(crate) fn new(scores: Vec<(String, f32)>) -> Self {
        Self { scores }
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
        let mut text =
            String::from("Jev tool selection for this turn — prefer these tools when calling:\n");
        for (name, prob) in &self.scores {
            let percent = (prob * 100.0).round() as i64;
            let line = format!("- `{name}` — {percent}%\n");
            if text.len() + line.len() + "</jev_tool_priority>".len() > MAX_HINT_CHARS {
                break;
            }
            text.push_str(&line);
        }
        text.push_str("Other MCP tools remain available via tool_search.\n");
        format!("\n{text}")
    }
}

#[cfg(test)]
#[path = "jev_tool_priority_hint_tests.rs"]
mod tests;

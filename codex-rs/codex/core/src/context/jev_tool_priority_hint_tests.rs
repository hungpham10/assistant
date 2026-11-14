//! Tests cho `JevToolPriorityHint` — body format và cap độ dài.

use super::JevToolPriorityHint;
use crate::context::ContextualUserFragment;
use pretty_assertions::assert_eq;

#[test]
fn body_lists_tools_as_ranked_preferences() {
    let hint = JevToolPriorityHint::new(vec!["tool_a".to_string(), "tool_b".to_string()]);
    let body = hint.body();
    assert_eq!(
        body,
        "\nTool routing for this turn — call these tools first, in this order of preference:\n\
         1. `tool_a`\n\
         2. `tool_b`\n\
         Every other tool is NOT attached this turn; to use another tool, first call `tool_search`.\n"
    );
}

#[test]
fn body_does_not_render_scores() {
    let hint = JevToolPriorityHint::new(vec!["tool".to_string()]);
    assert!(!hint.body().contains('%'));
}

#[test]
fn render_wraps_body_in_markers() {
    let hint = JevToolPriorityHint::new(vec!["tool".to_string()]);
    let rendered = hint.render();
    assert!(rendered.starts_with("<jev_tool_priority>"));
    assert!(rendered.trim_end().ends_with("</jev_tool_priority>"));
    assert_eq!(hint.role(), "developer");
}

#[test]
fn body_marks_truncation_and_stays_bounded() {
    let long_name = "x".repeat(200);
    let hint = JevToolPriorityHint::new(std::iter::repeat_n(long_name, 30).collect::<Vec<_>>());
    let rendered = hint.render();
    assert!(
        rendered.len()
            <= super::MAX_HINT_CHARS + "<jev_tool_priority>".len() + "</jev_tool_priority>".len(),
        "rendered hint must stay bounded: {}",
        rendered.len()
    );
    // At least one tool line survived the cap and the truncation is visible.
    assert!(rendered.contains("(list truncated)"));
}

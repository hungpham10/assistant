//! Tests cho `JevToolPriorityHint` — body format và cap độ dài.

use super::JevToolPriorityHint;
use crate::context::ContextualUserFragment;
use pretty_assertions::assert_eq;

#[test]
fn body_lists_tools_with_percentages_in_order() {
    let hint = JevToolPriorityHint::new(vec![
        ("srv/tool_a".to_string(), 0.852f32),
        ("srv/tool_b".to_string(), 0.61f32),
    ]);
    let body = hint.body();
    assert_eq!(
        body,
        "\nJev tool selection for this turn — prefer these tools when calling:\n\
         - `srv/tool_a` — 85%\n\
         - `srv/tool_b` — 61%\n\
         Other MCP tools remain available via tool_search.\n"
    );
}

#[test]
fn render_wraps_body_in_markers() {
    let hint = JevToolPriorityHint::new(vec![("srv/tool".to_string(), 0.5f32)]);
    let rendered = hint.render();
    assert!(rendered.starts_with("<jev_tool_priority>"));
    assert!(rendered.trim_end().ends_with("</jev_tool_priority>"));
    assert_eq!(hint.role(), "developer");
}

#[test]
fn body_stops_before_exceeding_cap() {
    let long_name = "srv/".to_string() + &"x".repeat(200);
    let hint =
        JevToolPriorityHint::new(std::iter::repeat_n((long_name, 0.5f32), 30).collect::<Vec<_>>());
    let rendered = hint.render();
    assert!(
        rendered.len()
            <= super::MAX_HINT_CHARS + "<jev_tool_priority>".len() + "</jev_tool_priority>".len(),
        "rendered hint must stay bounded: {}",
        rendered.len()
    );
    // At least one tool line survived the cap.
    assert!(rendered.contains("— 50%"));
}

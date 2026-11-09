use super::*;

#[test]
fn counts_calls_and_errors_with_dedupe() {
    let mut tracker = ToolUsageTracker::new();
    tracker.record_call("shell", "item-1", false, "turn-1");
    tracker.record_call("shell", "item-2", true, "turn-1");
    // Replay of the same item id must not double-count.
    tracker.record_call("shell", "item-1", false, "turn-1");
    tracker.record_call("mcp:srv/srv", "item-3", false, "turn-2");

    let shell = tracker.lookup("shell").expect("shell entry");
    assert_eq!(shell.calls, 2);
    assert_eq!(shell.errors, 1);
    assert_eq!(shell.last_turn_id.as_deref(), Some("turn-1"));
    assert!(shell.last_used_at.is_some());
}

#[test]
fn top_sorts_by_calls_then_jev_suggestions() {
    let mut tracker = ToolUsageTracker::new();
    tracker.record_call("a", "1", false, "");
    tracker.record_call("b", "2", false, "");
    tracker.record_call("b", "3", false, "");
    tracker.record_jev_suggestions(&["a".to_string()]);

    let top = tracker.top(2);
    assert_eq!(top[0].name, "b");
    assert_eq!(top[1].name, "a");
    assert_eq!(top[1].jev_suggestions, 1);
}

#[test]
fn suggested_but_unused_excludes_called_tools() {
    let mut tracker = ToolUsageTracker::new();
    tracker.record_jev_suggestions(&["srv/tool".to_string(), "srv/unused".to_string()]);
    tracker.record_call("srv/tool", "1", false, "");

    let unused = tracker.suggested_but_unused(5);
    assert_eq!(unused.len(), 1);
    assert_eq!(unused[0].name, "srv/unused");
}

#[test]
fn lookup_accepts_qualified_and_bare_names() {
    let mut tracker = ToolUsageTracker::new();
    tracker.record_call("codegraph/codegraph_search_symbol", "1", false, "");

    assert!(
        tracker
            .lookup("codegraph/codegraph_search_symbol")
            .is_some()
    );
    assert!(
        tracker
            .lookup("mcp:codegraph/codegraph_search_symbol")
            .is_some()
    );
    assert!(tracker.lookup("codegraph_search_symbol").is_some());
    assert!(tracker.lookup("nope").is_none());
    assert!(tracker.lookup("").is_none());
}

#[test]
fn extract_tool_call_skips_in_progress_items() {
    let make_item = |status| ThreadItem::McpToolCall {
        id: "1".to_string(),
        server: "codegraph".to_string(),
        tool: "codegraph_search_symbol".to_string(),
        status,
        arguments: serde_json::Value::Null,
        app_context: None,
        mcp_app_resource_uri: None,
        plugin_id: None,
        result: None,
        error: None,
        duration_ms: None,
    };
    let in_progress = make_item(McpToolCallStatus::InProgress);
    assert!(extract_tool_call(&in_progress).is_none());

    let done = make_item(McpToolCallStatus::Completed);
    let (name, id, errored) = extract_tool_call(&done).expect("extracted");
    assert_eq!(name, "codegraph/codegraph_search_symbol");
    assert_eq!(id, "1");
    assert!(!errored);
}

//! Tests for the `/tools` picker: namespaced grouping and the inventory load
//! state machine (an empty inventory must refetch on reopen, not loop).

use codex_app_server_protocol::ThreadToolInventoryItem;
use codex_app_server_protocol::ThreadToolSource;

use super::*;

fn core_builtin(name: &str) -> ThreadToolInventoryItem {
    ThreadToolInventoryItem {
        key: name.to_string(),
        namespace: None,
        name: name.to_string(),
        source: ThreadToolSource::Builtin,
        server_name: None,
        advertised: true,
        hidden: false,
        exposure: "direct".to_string(),
        description: Some("builtin".to_string()),
        spec: None,
    }
}

fn namespaced_builtin(namespace: &str, name: &str) -> ThreadToolInventoryItem {
    ThreadToolInventoryItem {
        key: format!("{namespace}:{name}"),
        namespace: Some(namespace.to_string()),
        name: name.to_string(),
        source: ThreadToolSource::Builtin,
        server_name: None,
        advertised: true,
        hidden: false,
        exposure: "direct".to_string(),
        description: Some("builtin".to_string()),
        spec: None,
    }
}

fn mcp_tool(server: &str, name: &str) -> ThreadToolInventoryItem {
    ThreadToolInventoryItem {
        key: format!("{server}:{name}"),
        namespace: Some(server.to_string()),
        name: name.to_string(),
        source: ThreadToolSource::Mcp,
        server_name: Some(server.to_string()),
        advertised: true,
        hidden: false,
        exposure: "direct".to_string(),
        description: Some(format!("mcp ({server})")),
        spec: None,
    }
}

/// Drain events until the next inventory fetch, returning its thread id.
fn next_fetch_thread_id(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Option<ThreadId> {
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::FetchThreadToolsInventory { thread_id } = event {
            return Some(thread_id);
        }
    }
    None
}

#[tokio::test]
async fn empty_inventory_refetches_on_reopen_without_loop() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);

    chat.open_tools_picker();
    assert_eq!(
        next_fetch_thread_id(&mut rx),
        Some(thread_id),
        "first open must fetch the inventory"
    );

    // Load completes with an empty inventory (no turn ran yet): the picker
    // re-renders the builtin catalog without kicking off another fetch.
    chat.on_thread_tools_inventory_loaded(thread_id, Vec::new());
    assert_eq!(
        next_fetch_thread_id(&mut rx),
        None,
        "auto re-render after an empty load must not refetch"
    );

    // Reopening `/tools` refetches (a turn may have registered tools since).
    chat.open_tools_picker();
    assert_eq!(
        next_fetch_thread_id(&mut rx),
        Some(thread_id),
        "reopen after an empty inventory must refetch"
    );
}

#[tokio::test]
async fn tools_picker_groups_namespaced_builtins_and_mcp() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);

    // Simulate a completed inventory fetch (a turn ran / MCP servers
    // connected): core builtins, codegraph-namespaced builtins, and MCP tools.
    chat.on_thread_tools_inventory_loaded(
        thread_id,
        vec![
            core_builtin("shell"),
            core_builtin("apply_patch"),
            namespaced_builtin("codegraph", "call"),
            namespaced_builtin("codegraph", "search_symbol"),
            mcp_tool("codegraph", "codegraph_init"),
        ],
    );

    chat.open_tools_picker();
    let popup = render_bottom_popup(&chat, /*width*/ 80);
    assert_chatwidget_snapshot!("tools_picker_namespaced_groups", popup);
}

#[tokio::test]
async fn tools_picker_recovers_after_empty_first_load() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);

    chat.open_tools_picker();
    assert_eq!(next_fetch_thread_id(&mut rx), Some(thread_id));
    chat.on_thread_tools_inventory_loaded(thread_id, Vec::new());

    chat.open_tools_picker();
    assert_eq!(next_fetch_thread_id(&mut rx), Some(thread_id));
    // This time tools arrive (a turn registered them): the auto re-render
    // shows the grouped picker.
    chat.on_thread_tools_inventory_loaded(
        thread_id,
        vec![
            core_builtin("shell"),
            namespaced_builtin("codegraph", "call"),
            mcp_tool("codegraph", "codegraph_init"),
        ],
    );

    let popup = render_bottom_popup(&chat, /*width*/ 80);
    assert_chatwidget_snapshot!("tools_picker_recovers_after_empty_first_load", popup);
}

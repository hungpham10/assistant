use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionToggle;
use crate::bottom_pane::SelectionViewParams;

use codex_app_server_protocol::ThreadToolInventoryItem;
use codex_app_server_protocol::ThreadToolSource;
use codex_protocol::ThreadId;

use super::BUILTIN_TOOL_CATALOG;
use super::ChatWidget;

impl ChatWidget {
    pub(crate) fn request_thread_tools_inventory_fetch(&mut self, thread_id: ThreadId) {
        if self.tools_picker_loading_thread == Some(thread_id) {
            return;
        }
        if self
            .thread_tools_inventory
            .as_ref()
            .is_some_and(|(cached_thread_id, _)| *cached_thread_id == thread_id)
        {
            return;
        }
        self.tools_picker_loading_thread = Some(thread_id);
        self.app_event_tx
            .send(AppEvent::FetchThreadToolsInventory { thread_id });
    }

    pub(crate) fn on_thread_tools_inventory_loaded(
        &mut self,
        thread_id: ThreadId,
        items: Vec<ThreadToolInventoryItem>,
    ) {
        let was_loading = self.tools_picker_loading_thread == Some(thread_id);
        if was_loading {
            self.tools_picker_loading_thread = None;
        }
        if self.thread_id() == Some(thread_id) {
            self.thread_tools_inventory = Some((thread_id, items));
            if was_loading && self.tools_picker_refresh_on_load {
                self.tools_picker_refresh_on_load = false;
                if self.bottom_pane.has_active_view() {
                    self.open_tools_picker();
                }
            } else if was_loading {
                self.tools_picker_refresh_on_load = false;
            }
        } else if was_loading {
            self.tools_picker_refresh_on_load = false;
        }
    }

    pub(crate) fn on_thread_tools_inventory_load_failed(&mut self, thread_id: ThreadId) {
        if self.tools_picker_loading_thread == Some(thread_id) {
            self.tools_picker_loading_thread = None;
            self.tools_picker_refresh_on_load = false;
        }
    }

    pub(crate) fn open_tools_picker(&mut self) {
        let thread_id = self.thread_id();
        let cached = thread_id.and_then(|tid| {
            self.thread_tools_inventory
                .as_ref()
                .filter(|(cached_thread_id, _)| *cached_thread_id == tid)
                .map(|(_, items)| items.as_slice())
        });

        let (items, subtitle) = if let Some(inventory) = cached {
            (
                build_tools_picker_items(self, inventory),
                "Disabled tools stay callable via tool_search. Space or Enter toggles; Esc closes."
                    .to_string(),
            )
        } else {
            if let Some(tid) = thread_id {
                self.request_thread_tools_inventory_fetch(tid);
                self.tools_picker_refresh_on_load = true;
            }
            let mut items: Vec<SelectionItem> = BUILTIN_TOOL_CATALOG
                .iter()
                .map(|name| builtin_selection_item(self, (*name).to_string(), (*name).to_string()))
                .collect();
            if thread_id.is_some() {
                items.push(SelectionItem {
                    name: "Loading MCP tools…".to_string(),
                    description: Some("Reopen /tools if this does not update.".to_string()),
                    dismiss_on_select: false,
                    ..Default::default()
                });
            }
            let subtitle = if thread_id.is_some() {
                "Loading MCP tools for this thread. Space or Enter toggles built-ins; Esc closes."
                    .to_string()
            } else {
                "Disabled tools stay callable via tool_search. Space or Enter toggles; Esc closes."
                    .to_string()
            };
            (items, subtitle)
        };

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Tools — enable/disable for this session".to_string()),
            subtitle: Some(subtitle),
            items,
            ..Default::default()
        });
        self.request_redraw();
    }
}

fn build_tools_picker_items(
    widget: &ChatWidget,
    inventory: &[ThreadToolInventoryItem],
) -> Vec<SelectionItem> {
    let mut builtin_by_key = std::collections::HashMap::new();
    let mut mcp = Vec::new();

    for item in inventory {
        match item.source {
            ThreadToolSource::Builtin => {
                builtin_by_key.insert(item.key.as_str(), item);
            }
            ThreadToolSource::Mcp => mcp.push(item),
            ThreadToolSource::Extension | ThreadToolSource::Dynamic | ThreadToolSource::Hosted => {}
        }
    }

    mcp.sort_by(|a, b| {
        display_server_name(a.server_name.as_deref())
            .cmp(display_server_name(b.server_name.as_deref()))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.key.cmp(&b.key))
    });

    let mut items = Vec::new();
    for name in BUILTIN_TOOL_CATALOG {
        if let Some(item) = builtin_by_key.get(*name) {
            items.push(tool_inventory_selection_item(widget, item));
        } else {
            items.push(builtin_selection_item(
                widget,
                (*name).to_string(),
                (*name).to_string(),
            ));
        }
    }
    for item in mcp {
        items.push(tool_inventory_selection_item(widget, item));
    }
    items
}

fn display_server_name(server_name: Option<&str>) -> &str {
    server_name
        .and_then(|name| name.strip_prefix("mcp__"))
        .or(server_name)
        .unwrap_or("mcp")
}

fn tool_inventory_selection_item(
    _widget: &ChatWidget,
    item: &ThreadToolInventoryItem,
) -> SelectionItem {
    let key = item.key.clone();
    let display_name = match item.source {
        ThreadToolSource::Builtin => item.name.clone(),
        ThreadToolSource::Mcp => format!(
            "{} / {}",
            display_server_name(item.server_name.as_deref()),
            item.name
        ),
        ThreadToolSource::Extension | ThreadToolSource::Dynamic | ThreadToolSource::Hosted => {
            item.key.clone()
        }
    };
    let description = match item.source {
        ThreadToolSource::Builtin => Some("builtin".to_string()),
        ThreadToolSource::Mcp => Some(format!(
            "mcp ({})",
            display_server_name(item.server_name.as_deref())
        )),
        ThreadToolSource::Extension => Some("extension".to_string()),
        ThreadToolSource::Dynamic => Some("dynamic".to_string()),
        ThreadToolSource::Hosted => Some("hosted".to_string()),
    };
    let enabled = !item.hidden;
    toggle_selection_item(display_name, key, enabled, description)
}

fn builtin_selection_item(widget: &ChatWidget, display_name: String, key: String) -> SelectionItem {
    let enabled = !widget.tools_session_disabled.contains(&key);
    toggle_selection_item(display_name, key, enabled, Some("builtin".to_string()))
}

fn toggle_selection_item(
    display_name: String,
    key: String,
    enabled: bool,
    description: Option<String>,
) -> SelectionItem {
    let enter_key = key.clone();
    SelectionItem {
        name: display_name,
        toggle: Some(SelectionToggle {
            is_on: enabled,
            action: Box::new(move |_enabled, tx: &AppEventSender| {
                tx.send(AppEvent::ToggleToolEnabled { name: key.clone() });
            }),
        }),
        description,
        actions: vec![Box::new(move |tx: &AppEventSender| {
            tx.send(AppEvent::ToggleToolEnabled {
                name: enter_key.clone(),
            });
        })],
        dismiss_on_select: false,
        ..Default::default()
    }
}

use codex_protocol::ToolName;
use codex_tools::ToolExposure;
use codex_tools::ToolSpec;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolInventorySource {
    Builtin,
    Mcp,
    Extension,
    Dynamic,
    Hosted,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInventoryEntry {
    pub key: String,
    pub namespace: Option<String>,
    pub name: String,
    pub source: ToolInventorySource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    pub advertised: bool,
    pub hidden: bool,
    pub exposure: ToolExposure,
    pub description: Option<String>,
}

pub fn tool_inventory_key(name: &ToolName) -> String {
    match &name.namespace {
        Some(namespace) => format!("{namespace}:{}", name.name),
        None => name.name.clone(),
    }
}

pub fn description_for_spec(spec: &ToolSpec) -> Option<String> {
    match spec {
        ToolSpec::Function(tool) => {
            if tool.description.trim().is_empty() {
                None
            } else {
                Some(tool.description.clone())
            }
        }
        ToolSpec::Namespace(namespace) => {
            if namespace.description.trim().is_empty() {
                None
            } else {
                Some(namespace.description.clone())
            }
        }
        ToolSpec::ToolSearch { .. }
        | ToolSpec::ImageGeneration { .. }
        | ToolSpec::WebSearch { .. }
        | ToolSpec::LocalShell {}
        | ToolSpec::Freeform(_) => None,
    }
}

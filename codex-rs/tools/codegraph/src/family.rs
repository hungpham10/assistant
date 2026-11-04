use codex_enrich::command::CommandParser;
use codex_enrich::component::BoxedComponent;
use codex_enrich::family::{ComponentSpec, FamilyPolicy, ToolFamily};
use std::sync::Arc;

pub struct CodegraphFamily;

impl ToolFamily for CodegraphFamily {
    fn namespace(&self) -> &str {
        "codegraph"
    }

    fn policy(&self, _cfg: &serde_json::Value) -> FamilyPolicy {
        FamilyPolicy {
            enabled: true,
            permit_modes: vec!["lex".to_string()],
            deferred: true,
        }
    }

    fn command_syntax(&self) -> Option<Arc<dyn CommandParser>> {
        None
    }

    fn components(&self) -> Vec<ComponentSpec> {
        Vec::new()
    }

    fn build_components(&self, _cfg: &serde_json::Value) -> Vec<BoxedComponent> {
        Vec::new()
    }
}

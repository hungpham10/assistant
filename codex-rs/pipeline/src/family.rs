use crate::command::CommandParser;
use crate::component::BoxedComponent;
use serde_json::Value as JsonValue;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct FamilyPolicy {
    pub enabled: bool,
    pub permit_modes: Vec<String>,
    pub deferred: bool,
}

#[derive(Debug, Clone)]
pub struct ComponentSpec {
    pub id: String,
    pub r#type: String,
    pub config: JsonValue,
}

pub trait ToolFamily: Send + Sync {
    fn namespace(&self) -> &str;
    fn policy(&self, _cfg: &JsonValue) -> FamilyPolicy {
        FamilyPolicy {
            enabled: true,
            permit_modes: Vec::new(),
            deferred: true,
        }
    }
    fn command_syntax(&self) -> Option<Arc<dyn CommandParser>> {
        None
    }
    fn components(&self) -> Vec<ComponentSpec> {
        Vec::new()
    }
    fn build_components(&self, _cfg: &JsonValue) -> Vec<BoxedComponent> {
        Vec::new()
    }
}

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

/// Declares one family verb as a model-visible tool.
///
/// `parameters` is a JSON Schema object describing the verb's arguments;
/// `defer_loading` hides the verb from the always-on tool list and exposes it
/// only through tool search.
#[derive(Debug, Clone)]
pub struct VerbSpec {
    pub verb: String,
    pub description: String,
    pub parameters: JsonValue,
    pub defer_loading: bool,
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
    /// Verbs this family wants exposed to the model as callable tools.
    /// Families default to command-syntax-only exposure.
    fn verb_specs(&self) -> Vec<VerbSpec> {
        Vec::new()
    }
    fn run_verb(
        &self,
        _verb: &str,
        _args: &JsonValue,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<JsonValue>> + Send + '_>>
    {
        Box::pin(async { Ok(serde_json::json!({})) })
    }
}

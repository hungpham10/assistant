use serde_json::Value as JsonValue;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct ComponentStatus {
    pub running: bool,
    pub last_error: Option<String>,
    pub fault_count: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Enrichment {
    pub source: String,
    pub data: JsonValue,
    pub token_estimate: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct StreamMessage {
    pub prompt: String,
    pub commands: Vec<crate::command::VerbCall>,
    pub evidence: Vec<Enrichment>,
    pub hints: Vec<String>,
}

pub trait Component: Send + Sync {
    fn id(&self) -> &'static str;
    fn run<'a>(
        &'a self,
        msg: StreamMessage,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<StreamMessage>> + Send + 'a>>;
    fn status(&self) -> ComponentStatus;
}

pub type BoxedComponent = Arc<dyn Component>;

#[derive(Debug, Clone, Default)]
pub struct TapConfig {
    pub components: Vec<String>,
    pub token_budget: usize,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineProfile {
    pub components: Vec<crate::family::ComponentSpec>,
    pub taps: std::collections::HashMap<String, TapConfig>,
}


#[derive(Debug, Clone)]
pub struct CommandResult {
    pub call: crate::command::VerbCall,
    pub output: serde_json::Value,
}

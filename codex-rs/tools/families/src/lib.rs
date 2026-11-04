use codex_pipeline::command::{AtCommandParser, CommandParser};
use codex_pipeline::component::{BoxedComponent, Component, ComponentStatus, StreamMessage};
use codex_pipeline::family::{FamilyPolicy, ToolFamily};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[derive(Debug)]
pub struct NoopFamily;

impl ToolFamily for NoopFamily {
    fn namespace(&self) -> &str {
        "noop"
    }

    fn policy(&self, _cfg: &serde_json::Value) -> FamilyPolicy {
        FamilyPolicy {
            enabled: true,
            permit_modes: vec!["default".to_string()],
            deferred: false,
        }
    }

    fn command_syntax(&self) -> Option<Arc<dyn CommandParser>> {
        Some(Arc::new(AtCommandParser))
    }

    fn build_components(&self, _cfg: &serde_json::Value) -> Vec<BoxedComponent> {
        vec![Arc::new(NoopComponent)]
    }
}

#[derive(Debug)]
struct NoopComponent;

impl Component for NoopComponent {
    fn id(&self) -> &'static str {
        "noop.echo"
    }

    fn run<'a>(
        &'a self,
        mut msg: StreamMessage,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<StreamMessage>> + Send + 'a>> {
        Box::pin(async move {
            msg.hints.push("noop component executed".to_string());
            Ok(msg)
        })
    }

    fn status(&self) -> ComponentStatus {
        ComponentStatus {
            running: false,
            last_error: None,
            fault_count: 0,
        }
    }
}

#[derive(Debug)]
pub struct CodeModeFamily;

impl ToolFamily for CodeModeFamily {
    fn namespace(&self) -> &str {
        "code_mode"
    }

    fn policy(&self, _cfg: &serde_json::Value) -> FamilyPolicy {
        FamilyPolicy {
            enabled: true,
            permit_modes: Vec::new(),
            deferred: true,
        }
    }

    fn command_syntax(&self) -> Option<Arc<dyn CommandParser>> {
        Some(Arc::new(AtCommandParser))
    }

    fn build_components(&self, _cfg: &serde_json::Value) -> Vec<BoxedComponent> {
        Vec::new()
    }
}

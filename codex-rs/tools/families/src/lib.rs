use codex_pipeline::command::{AtCommandParser, CommandParser};
use codex_pipeline::component::{BoxedComponent, Component, ComponentStatus, StreamMessage};
use codex_pipeline::family::{FamilyPolicy, ToolFamily};
use serde_json::Value as JsonValue;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[derive(Debug)]
pub struct NoopFamily;

impl ToolFamily for NoopFamily {
    fn namespace(&self) -> &str {
        "noop"
    }

    fn policy(&self, _cfg: &JsonValue) -> FamilyPolicy {
        FamilyPolicy {
            enabled: true,
            permit_modes: vec!["default".to_string()],
            deferred: false,
        }
    }

    fn command_syntax(&self) -> Option<Arc<dyn CommandParser>> {
        Some(Arc::new(AtCommandParser))
    }

    fn build_components(&self, _cfg: &JsonValue) -> Vec<BoxedComponent> {
        vec![Arc::new(NoopComponent)]
    }

    fn run_verb(
        &self,
        verb: &str,
        args: &JsonValue,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<JsonValue>> + Send + '_>> {
        let verb = verb.to_string();
        let args = args.clone();
        Box::pin(async move {
            Ok(serde_json::json!({
                "verb": verb,
                "args": args,
                "this": { "id": 42, "name": "noop-result" },
                "ok": true
            }))
        })
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

    fn policy(&self, _cfg: &JsonValue) -> FamilyPolicy {
        FamilyPolicy {
            enabled: true,
            permit_modes: Vec::new(),
            deferred: true,
        }
    }

    fn command_syntax(&self) -> Option<Arc<dyn CommandParser>> {
        Some(Arc::new(AtCommandParser))
    }

    fn build_components(&self, _cfg: &JsonValue) -> Vec<BoxedComponent> {
        Vec::new()
    }
}

use crate::command::AtCommandParser;
use crate::command::CommandParser;
use crate::component::BoxedComponent;
use crate::component::Component;
use crate::component::ComponentStatus;
use crate::component::StreamMessage;
use crate::family::FamilyPolicy;
use crate::family::ToolFamily;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::FamilyRegistry;

    #[test]
    fn test_noop_family_registration() {
        let mut registry = FamilyRegistry::new();
        registry.register(Arc::new(NoopFamily));
        assert_eq!(registry.families().len(), 1);
        let family = registry.get("noop").unwrap();
        assert_eq!(family.namespace(), "noop");
        let policy = family.policy(&serde_json::json!({}));
        assert!(policy.enabled);
    }

    #[tokio::test]
    async fn test_noop_component() {
        let family = NoopFamily;
        let components = family.build_components(&serde_json::json!({}));
        assert_eq!(components.len(), 1);
        let msg = StreamMessage::default();
        let result = components[0].run(msg).await.unwrap();
        assert!(!result.hints.is_empty());
    }
}

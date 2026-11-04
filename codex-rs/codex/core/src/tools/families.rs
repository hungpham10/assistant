use codex_pipeline::noop::NoopFamily;
use codex_pipeline::registry::FamilyRegistry;
use std::sync::Arc;

pub fn build_family_registry() -> FamilyRegistry {
    let mut registry = FamilyRegistry::new();
    registry.register(Arc::new(NoopFamily));
    registry
}

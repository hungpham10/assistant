use codex_pipeline::registry::FamilyRegistry;
use codex_tools_families::CodeModeFamily;
use codex_tools_families::NoopFamily;
use std::sync::Arc;

pub fn build_family_registry() -> FamilyRegistry {
    let mut registry = FamilyRegistry::new();
    registry.register(Arc::new(NoopFamily));
    registry.register(Arc::new(CodeModeFamily));
    registry
}

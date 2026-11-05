use codex_enrich::registry::FamilyRegistry;
use codex_tools::families::CodeModeFamily;
use codex_tools::families::NoopFamily;
use std::sync::Arc;

pub fn build_family_registry() -> FamilyRegistry {
    let mut registry = FamilyRegistry::new();
    registry.register(Arc::new(NoopFamily));
    registry.register(Arc::new(CodeModeFamily));
    registry
}

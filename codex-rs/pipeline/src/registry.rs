use crate::family::ToolFamily;
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct FamilyRegistry {
    families: Vec<Arc<dyn ToolFamily>>,
}

impl FamilyRegistry {
    pub fn new() -> Self {
        Self {
            families: Vec::new(),
        }
    }

    pub fn register(&mut self, family: Arc<dyn ToolFamily>) {
        self.families.push(family);
    }

    pub fn families(&self) -> &[Arc<dyn ToolFamily>] {
        &self.families
    }

    pub fn get(&self, namespace: &str) -> Option<Arc<dyn ToolFamily>> {
        self.families
            .iter()
            .find(|f| f.namespace() == namespace)
            .cloned()
    }
}

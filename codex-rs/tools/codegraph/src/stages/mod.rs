use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct StageStatus {
    pub running: bool,
    pub last_error: Option<String>,
    pub fault_count: u64,
}

pub trait Stage: Send + Sync {
    fn name(&self) -> &'static str;
    fn status(&self) -> StageStatus;
}

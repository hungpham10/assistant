use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub enum IndexState {
    Present,
    Indexing,
    Ready,
    Error(String),
}

pub struct CodegraphService {
    index_state: Arc<Mutex<IndexState>>,
}

impl CodegraphService {
    pub fn attach(_root: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            index_state: Arc::new(Mutex::new(IndexState::Present)),
        })
    }
}

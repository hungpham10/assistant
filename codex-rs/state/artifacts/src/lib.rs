//! Artifact store for pipeline outputs that are too large (or too binary)
//! to inline into the model context.
//!
//! Artifacts live under `$CODEX_HOME/artifacts/<thread_id>/`. Verbs return
//! an [`ArtifactRef`] instead of raw bytes; downstream verbs and the model
//! reference artifacts by id/path.

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use codex_path::absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Metadata for one stored artifact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub path: PathBuf,
    /// MIME-ish kind, e.g. `audio/wav`, `image/png`, `text/plain`.
    pub kind: String,
    #[serde(default)]
    pub meta: JsonValue,
}

/// Filesystem-backed artifact store rooted at a per-thread directory.
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: AbsolutePathBuf,
}

impl ArtifactStore {
    /// Creates the store directory `$CODEX_HOME/artifacts/<thread_id>/`.
    pub fn new(codex_home: &AbsolutePathBuf, thread_id: &str) -> io::Result<Self> {
        let root = codex_home.join("artifacts").join(thread_id);
        std::fs::create_dir_all(root.as_path())?;
        Ok(Self { root })
    }

    /// Directory that backs this store (also where `$CODEX_HOME/artifacts`
    /// sweeps start).
    pub fn root(&self) -> &AbsolutePathBuf {
        &self.root
    }

    /// Stores bytes as a new artifact and returns its reference.
    pub async fn put(
        &self,
        bytes: Vec<u8>,
        kind: &str,
        meta: JsonValue,
    ) -> io::Result<ArtifactRef> {
        let artifact_id = format!("a-{}", Uuid::new_v4().simple());
        let path = self.root.join(&artifact_id);
        tokio::fs::write(path.as_path(), bytes).await?;
        Ok(ArtifactRef {
            artifact_id,
            path: path.into_path_buf(),
            kind: kind.to_string(),
            meta,
        })
    }

    /// Moves an existing file into the store, returning its reference.
    pub async fn put_from_path(
        &self,
        src: &Path,
        kind: &str,
        meta: JsonValue,
    ) -> io::Result<ArtifactRef> {
        let bytes = tokio::fs::read(src).await?;
        self.put(bytes, kind, meta).await
    }

    /// Reads an artifact's bytes back.
    pub async fn read(&self, artifact_ref: &ArtifactRef) -> io::Result<Vec<u8>> {
        tokio::fs::read(&artifact_ref.path).await
    }

    /// Deletes artifacts older than `ttl`. Returns the number removed.
    pub async fn cleanup_expired(&self, ttl: Duration) -> io::Result<usize> {
        let mut removed = 0usize;
        let now = std::time::SystemTime::now();
        let mut entries = tokio::fs::read_dir(self.root.as_path()).await?;
        while let Some(entry) = entries.next_entry().await? {
            let Ok(modified) = entry.metadata().await.and_then(|m| m.modified()) else {
                continue;
            };
            if let Ok(age) = now.duration_since(modified)
                && age > ttl
            {
                if tokio::fs::remove_file(entry.path()).await.is_ok() {
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
#[path = "artifacts_tests.rs"]
mod tests;

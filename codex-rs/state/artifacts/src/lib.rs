//! Artifact store for pipeline outputs that are too large (or too binary)
//! to inline into the model context.
//!
//! Artifacts live under `$CODEX_HOME/artifacts/<thread_id>/`. Verbs return
//! an [`ArtifactRef`] instead of raw bytes; downstream verbs and the model
//! reference artifacts by id/path.

use std::future::Future;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use codex_path::absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Future returned by [`ArtifactStoreBackend`] operations.
pub type ArtifactStoreBackendFuture<'a, T> =
    Pin<Box<dyn Future<Output = io::Result<T>> + Send + 'a>>;

/// Abstract storage backend behind [`ArtifactStore`]. The default
/// [`LocalArtifactStoreBackend`] writes to the local filesystem; an alternative
/// backend (for example, a remote object store) can be injected via
/// [`ArtifactStore::with_backend`].
pub trait ArtifactStoreBackend: Send + Sync + std::fmt::Debug {
    /// Writes `contents` to `path`, replacing it if it already exists.
    fn write<'a>(&'a self, path: &'a Path, contents: Vec<u8>)
    -> ArtifactStoreBackendFuture<'a, ()>;

    /// Reads the full contents of `path`.
    fn read<'a>(&'a self, path: &'a Path) -> ArtifactStoreBackendFuture<'a, Vec<u8>>;

    /// Returns each stored entry's path and last-modified time. Entries whose
    /// modified time cannot be determined are omitted.
    fn entries<'a>(&'a self) -> ArtifactStoreBackendFuture<'a, Vec<(PathBuf, SystemTime)>>;

    /// Removes the file at `path`.
    fn remove<'a>(&'a self, path: &'a Path) -> ArtifactStoreBackendFuture<'a, ()>;
}

/// [`ArtifactStoreBackend`] backed by the local filesystem via `tokio::fs`.
#[derive(Debug, Clone)]
pub struct LocalArtifactStoreBackend {
    root: AbsolutePathBuf,
}

impl ArtifactStoreBackend for LocalArtifactStoreBackend {
    fn write<'a>(
        &'a self,
        path: &'a Path,
        contents: Vec<u8>,
    ) -> ArtifactStoreBackendFuture<'a, ()> {
        Box::pin(async move { tokio::fs::write(path, contents).await })
    }

    fn read<'a>(&'a self, path: &'a Path) -> ArtifactStoreBackendFuture<'a, Vec<u8>> {
        Box::pin(async move { tokio::fs::read(path).await })
    }

    fn entries<'a>(&'a self) -> ArtifactStoreBackendFuture<'a, Vec<(PathBuf, SystemTime)>> {
        Box::pin(async {
            let mut entries = Vec::new();
            let mut read_dir = tokio::fs::read_dir(self.root.as_path()).await?;
            while let Some(entry) = read_dir.next_entry().await? {
                if let Ok(modified) = entry.metadata().await.and_then(|m| m.modified()) {
                    entries.push((entry.path(), modified));
                }
            }
            Ok(entries)
        })
    }

    fn remove<'a>(&'a self, path: &'a Path) -> ArtifactStoreBackendFuture<'a, ()> {
        Box::pin(async move { tokio::fs::remove_file(path).await })
    }
}

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

/// Artifact store rooted at a per-thread directory.
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: AbsolutePathBuf,
    backend: Arc<dyn ArtifactStoreBackend>,
}

impl ArtifactStore {
    /// Creates the store directory `$CODEX_HOME/artifacts/<thread_id>/`.
    pub fn new(codex_home: &AbsolutePathBuf, thread_id: &str) -> io::Result<Self> {
        let root = codex_home.join("artifacts").join(thread_id);
        std::fs::create_dir_all(root.as_path())?;
        Ok(Self::with_backend(
            root.clone(),
            Arc::new(LocalArtifactStoreBackend { root }),
        ))
    }

    /// Builds a store on top of an injected backend. Unlike [`ArtifactStore::new`],
    /// this does not create `root`; the backend is responsible for its own layout.
    pub fn with_backend(root: AbsolutePathBuf, backend: Arc<dyn ArtifactStoreBackend>) -> Self {
        Self { root, backend }
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
        self.backend.write(path.as_path(), bytes).await?;
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
        self.backend.read(&artifact_ref.path).await
    }

    /// Deletes artifacts older than `ttl`. Returns the number removed.
    pub async fn cleanup_expired(&self, ttl: Duration) -> io::Result<usize> {
        let mut removed = 0usize;
        let now = std::time::SystemTime::now();
        let root = self.root.as_path();
        for (path, modified) in self.backend.entries().await? {
            if path.parent() != Some(root) {
                continue;
            }
            if let Ok(age) = now.duration_since(modified)
                && age > ttl
                && self.backend.remove(&path).await.is_ok()
            {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
#[path = "artifacts_tests.rs"]
mod tests;

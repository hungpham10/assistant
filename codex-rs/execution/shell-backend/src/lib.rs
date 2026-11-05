//! Process backend abstraction for shell tool execution.
//!
//! Core spawns shell tool processes through [`ShellProcessBackend`] instead of
//! touching `tokio::process` directly, so a remote executor can be introduced
//! by implementing the trait without changing the tool layer.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use codex_network_proxy::NetworkProxy;
use codex_path::absolute_path::AbsolutePathBuf;
use codex_protocol::permissions::NetworkSandboxPolicy;

mod local;

pub use local::StdSystemProcessBackend;

/// Experimental environment variable that will be set to some non-empty value
/// if both of the following are true:
///
/// 1. The process was spawned by Codex as part of a shell tool call.
/// 2. NetworkSandboxPolicy is restricted for the tool call.
///
/// We may try to have just one environment variable for all sandboxing
/// attributes, so this may change in the future.
pub const CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR: &str = "CODEX_SANDBOX_NETWORK_DISABLED";

/// Should be set when the process is spawned under a sandbox. Currently, the
/// value is "seatbelt" for macOS, but it may change in the future to
/// accommodate sandboxing configuration and other sandboxing mechanisms.
pub const CODEX_SANDBOX_ENV_VAR: &str = "CODEX_SANDBOX";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdioPolicy {
    RedirectForShellTool,
    Inherit,
}

/// Everything a backend needs to launch a shell tool process. Owned by value
/// so backends can serialize or forward it.
#[derive(Debug, Clone)]
pub struct SpawnShellRequest {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub arg0: Option<String>,
    pub cwd: AbsolutePathBuf,
    pub network_sandbox_policy: NetworkSandboxPolicy,
    pub network: Option<std::sync::Arc<NetworkProxy>>,
    pub stdio_policy: StdioPolicy,
    pub env: HashMap<String, String>,
}

/// A running process started by a [`ShellProcessBackend`].
///
/// The concrete local implementation wraps a tokio child; remote implementations
/// can back the readers with network streams.
pub struct SpawnedShellProcess {
    child: tokio::process::Child,
}

impl SpawnedShellProcess {
    pub fn new(child: tokio::process::Child) -> Self {
        Self { child }
    }

    /// Takes the piped stdout reader, if the process was spawned with piped stdio.
    pub fn take_stdout(&mut self) -> Option<Box<dyn tokio::io::AsyncRead + Send + Unpin>> {
        self.child
            .stdout
            .take()
            .map(|reader| Box::new(reader) as Box<dyn tokio::io::AsyncRead + Send + Unpin>)
    }

    /// Takes the piped stderr reader, if the process was spawned with piped stdio.
    pub fn take_stderr(&mut self) -> Option<Box<dyn tokio::io::AsyncRead + Send + Unpin>> {
        self.child
            .stderr
            .take()
            .map(|reader| Box::new(reader) as Box<dyn tokio::io::AsyncRead + Send + Unpin>)
    }

    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    pub fn start_kill(&mut self) -> std::io::Result<()> {
        self.child.start_kill()
    }

    /// Kills every member of the process's process group.
    pub fn kill_process_group(&mut self) -> std::io::Result<()> {
        codex_utils_pty::process_group::kill_child_process_group(&mut self.child)
    }
}

/// Future returned by [`ShellProcessBackend::spawn`].
pub type ShellProcessBackendSpawnFuture<'a> =
    Pin<Box<dyn Future<Output = std::io::Result<SpawnedShellProcess>> + Send + 'a>>;

/// Backend that launches shell tool processes.
pub trait ShellProcessBackend: Send + Sync {
    fn spawn(&self, request: SpawnShellRequest) -> ShellProcessBackendSpawnFuture<'_>;
}

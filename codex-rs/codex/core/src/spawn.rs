//! Compatibility shim: process spawning lives in the `codex-shell-backend`
//! crate so a remote executor can replace the local backend via
//! [`ShellProcessBackend`] without touching the tool layer.

pub use codex_shell_backend::CODEX_SANDBOX_ENV_VAR;
pub use codex_shell_backend::CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR;
pub use codex_shell_backend::ShellProcessBackend;
pub use codex_shell_backend::SpawnShellRequest;
pub use codex_shell_backend::SpawnedShellProcess;
pub use codex_shell_backend::StdSystemProcessBackend;
pub use codex_shell_backend::StdioPolicy;

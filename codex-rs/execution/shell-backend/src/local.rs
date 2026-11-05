//! Local in-process shell process backend.
//!
//! This is the code moved verbatim from `codex-core/src/spawn.rs`; only the
//! ownership shape changed (owned request, boxed-future trait surface).

use std::process::Stdio;

use tokio::process::Command;
use tracing::trace;

use crate::CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR;
use crate::ShellProcessBackend;
use crate::ShellProcessBackendSpawnFuture;
use crate::SpawnShellRequest;
use crate::SpawnedShellProcess;
use crate::StdioPolicy;

/// Default backend: spawns the child process locally in this OS process.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdSystemProcessBackend;

impl ShellProcessBackend for StdSystemProcessBackend {
    fn spawn(&self, request: SpawnShellRequest) -> ShellProcessBackendSpawnFuture<'_> {
        Box::pin(async move { spawn_local(request).await })
    }
}

/// Spawns the appropriate child process for the exec params and sandbox settings,
/// ensuring the args and environment variables used to create the `Command`
/// (and `Child`) honor the configuration.
///
/// For now, we take `NetworkSandboxPolicy` as a parameter to spawn_child()
/// because we need to determine whether to set the
/// `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` environment variable.
async fn spawn_local(
    SpawnShellRequest {
        program,
        args,
        arg0,
        cwd,
        network_sandbox_policy,
        network,
        stdio_policy,
        mut env,
    }: SpawnShellRequest,
) -> std::io::Result<SpawnedShellProcess> {
    trace!(
        "spawn_child_async: {program:?} {args:?} {arg0:?} {cwd:?} {network_sandbox_policy:?} {stdio_policy:?} {env:?}"
    );

    let mut cmd = Command::new(&program);
    #[cfg(unix)]
    cmd.arg0(arg0.map_or_else(|| program.to_string_lossy().to_string(), String::from));
    cmd.args(args);
    cmd.current_dir(cwd);
    if let Some(network) = network {
        network.apply_to_env(&mut env);
    }
    cmd.env_clear();
    cmd.envs(env);

    if !network_sandbox_policy.is_enabled() {
        cmd.env(CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR, "1");
    }

    // If this Codex process dies (including being killed via SIGKILL), we want
    // any child processes that were spawned as part of a `"shell"` tool call
    // to also be terminated.

    #[cfg(unix)]
    unsafe {
        let detach_from_tty = matches!(stdio_policy, StdioPolicy::RedirectForShellTool);
        #[cfg(target_os = "linux")]
        let parent_pid = libc::getpid();
        cmd.pre_exec(move || {
            if detach_from_tty {
                codex_utils_pty::process_group::detach_from_tty()?;
            }

            // This relies on prctl(2), so it only works on Linux.
            #[cfg(target_os = "linux")]
            {
                // This prctl call effectively requests, "deliver SIGTERM when my
                // current parent dies."
                codex_utils_pty::process_group::set_parent_death_signal(parent_pid)?;
            }
            Ok(())
        });
    }

    match stdio_policy {
        StdioPolicy::RedirectForShellTool => {
            // Do not create a file descriptor for stdin because otherwise some
            // commands may hang forever waiting for input. For example, ripgrep has
            // a heuristic where it may try to read from stdin as explained here:
            // https://github.com/BurntSushi/ripgrep/blob/e2362d4d5185d02fa857bf381e7bd52e66fafc73/crates/core/flags/hiargs.rs#L1101-L1103
            cmd.stdin(Stdio::null());

            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        StdioPolicy::Inherit => {
            // Inherit stdin, stdout, and stderr from the parent process.
            cmd.stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
        }
    }

    Ok(SpawnedShellProcess::new(cmd.kill_on_drop(true).spawn()?))
}
